# HTTP/3 fallback under request cancellation

Date: 2026-08-04
Issue: PR #23 (failing regression test), issues #24 and #25 (related, separate)

## Problem

A long-lived agent stops completing any request after a transient UDP break, and stays that way. A downstream consumer saw 6.5+ minutes of total outage across multiple retry cycles, ending only when their pods were recycled — while TCP was healthy the entire time.

`AltSvcMiddleware::handle` is the only thing that can demote a confirmed HTTP/3 origin, and it can only do so on the `Err` arm of the h3 attempt:

```rust
let result = next.clone().run(req, extensions).await;   // dropped on abort
match result {
    Ok(response) => { /* confirm_h3 */ }
    Err(_) => { self.cache.record_h3_failure(&url); /* retry over TCP */ }
}
```

`fetch.rs` races `request.send()` against the abort signal in a `tokio::select!`. When the abort wins, that future is **dropped**, so neither arm runs. `record_h3_failure` is unreachable under cancellation. The `confirmed` Alt-Svc entry survives its 24h TTL, `should_use_h3` keeps returning `Some`, and every retry re-attempts h3 over the dead UDP path indefinitely.

The fallback decision lives inside the future being cancelled. State that must survive cancellation belongs in a `Drop` impl or outside the cancelled scope.

### Why it does not self-heal

There is one accidental escape hatch. A request holding a pooled h3 `SendRequest` at the instant quinn's idle timer kills the connection gets an immediate `Err`, which does reach `record_h3_failure`. That is a race, not a mechanism: it only fires if a request is in flight at that exact moment. A retry loop with backoff has gaps, so nothing is in flight; and once the dead connection is evicted, every later attempt is a fresh QUIC handshake that hangs ~30s and is cancelled first. No error ever surfaces.

This also explains the two corroborating observations: a fresh agent works immediately (empty Alt-Svc cache, so the first request goes over TCP), and node's `fetch` was unaffected (no HTTP/3 at all).

### Evidence

Reproduced against both a quinn h3 server and real Caddy 2.11.4. Same fault, same server, same retry loop; only the cancellation mechanism differs.

| Cancellation | quinn h3-server | Caddy 2.11.4 |
| --- | --- | --- |
| `AbortSignal` | 0/8 wedged | 0/6 wedged |
| `options.timeout` | 8/8 fell back | 6/6 fell back |
| `AbortSignal` + shorter `timeout` | 6/6 fell back | not run |

DNS is ruled out: one repro used a literal IP (resolution bypassed entirely), the other pinned `dns.overrides`, and the wedge reproduced in both.

## Goals

- A broken UDP path degrades to TCP within a bounded number of retries, regardless of how the caller cancels.
- Ordinary caller aborts of healthy requests do not disable HTTP/3.
- No change to abort semantics: cancelling a request still stops its work.
- No new dependencies.

## Non-goals

- Making cancelled requests succeed. Demotion affects the *next* request; the cancelled caller still sees `Aborted`.
- Honouring the advertised Alt-Svc port (issue #24).
- A multi-server conformance suite (issue #25).
- Changing `fetch.rs`. The `select!` keeps dropping the future; this design makes that drop observable instead.

## Approach

Two independent mechanisms, either of which breaks the wedge on its own.

### 1. Cancellation strikes (primary)

A cancelled h3 attempt is a **weak** signal, not a failure. It records a strike against the origin. Three consecutive strikes within a 60-second window demote the origin.

A hard failure per cancellation was rejected: a caller that routinely aborts healthy requests — speculative fetches, user navigation, request racing — would knock HTTP/3 out for `failed_ttl` (300s) at a time. Strikes reset on any successful h3 response, so a healthy origin interleaves successes that clear the evidence.

The 60-second window matters as much as the count. Pure consecutive counting would let three unrelated aborts spread over an hour demote an origin; requiring them inside a window encodes an actual incident.

The window is measured **from the previous strike, not from the first**: moka's `time_to_live` runs from the last write, and an upsert is a write, so each strike refreshes the entry. Three strikes therefore demote only if each lands within 60s of the one before it — a sustained incident rather than a fixed bucket. Two strikes followed by 60s of quiet decay to zero.

Because the guarded await resolves at *response headers*, an armed drop always means no h3 response arrived — when no cache store is configured. With `cache.store` set, the HTTP cache middleware buffers the full body inside that same guarded future (it sits inside `AltSvcMiddleware`, not outside it), so a body-phase abort can also reach the guard and record a strike in that configuration.

### 2. Attempt deadline (secondary)

An optional ceiling on how long the h3 attempt may take to produce response headers. On expiry, the attempt is treated as a real failure and retried over TCP immediately — no strikes needed.

Defaulted to 60s, deliberately high. quinn's `maxIdleTimeout` defaults to 30s and is capped at 120s, so at 60s the default never trips in normal operation: quinn's own error surfaces first. It only bites when someone raises `maxIdleTimeout` past 60s, acting as a safety net. Clients wanting fast recovery set 2–3s.

### Rejected alternatives

**Detached completion** — on abort, let the h3 attempt run to completion in the background so its true outcome is recorded. The most accurate signal, but it makes abort not actually abort: work and connections outlive the caller's cancellation, which is wrong for a fetch API, and a wedged origin accumulates detached attempts.

**Plumbing cancellation into the request future** so it surfaces as an `Err` rather than a drop. Conceptually the cleanest, since it fixes the whole class of bug. reqwest exposes no cancellation-token API, so it needs upstream changes.

## Design

### `AltSvcCache`

Two new fields:

```rust
cancellations: Cache<String, u32>,   // keyed by origin, TTL = strike window
cancel_strikes: u32,                 // default 3; 0 disables
```

The strike window is a constructor parameter rather than public config, defaulted to 60s by `Agent`. This exists so unit tests can pass a short window and assert decay without a time-mocking dependency.

Method changes:

- `record_h3_cancellation(&self, url)` — new. Returns immediately when `cancel_strikes == 0`. Otherwise atomically increments the origin's count via moka's `entry().and_upsert_with()`, which is per-key atomic so concurrent aborts cannot lose increments. On reaching `cancel_strikes`, clears the counter and delegates to `record_h3_failure`.

  A concurrent request can record a strike against an origin another request just demoted: its guard was armed while the origin was still confirmed. The resulting counter is stale but harmless, and decays with the window. No guarding against this is needed, because an origin already in `failed` makes `trying_h3` false, so no new guard is ever armed for it.
- `confirm_h3` — additionally invalidates the origin's count. A working h3 response is proof of health.
- `record_h3_failure` — additionally clears the count. The origin is already demoted, so further counting is meaningless.
- `Debug` impl — include the cancellation count alongside the existing counts.

Demotion reuses `record_h3_failure`, so the existing `failed` cache and `failed_ttl` do all the suppression work, exactly as a genuine h3 error would. Merely evicting `confirmed`/`advertised` was rejected: the next TCP response re-advertises h3, re-arming the wedge and oscillating indefinitely on a still-broken path.

### `AltSvcMiddleware`

One new field, `attempt_timeout: Option<Duration>`, and a guard:

```rust
struct H3AttemptGuard {
    cache: Arc<AltSvcCache>,
    url: Url,
    armed: bool,
}

impl Drop for H3AttemptGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cache.record_h3_cancellation(&self.url);
        }
    }
}
```

`moka::sync` is synchronous, so recording from `Drop` needs no async and is safe on a worker thread during cancellation. The `Drop` impl must be infallible: no panics, no unwrapping. It can run during unwind, where a panic would abort the process.

Control flow in the `trying_h3` branch:

1. Clone the request (unchanged), set version to HTTP/3.
2. Arm the guard.
3. Await the attempt, wrapped in `tokio::time::timeout` when `attempt_timeout` is set.
4. Disarm the guard. Reached on every resolved outcome, including deadline expiry.
5. Dispatch:
   - h3 response → `confirm_h3` (clears strikes), record any Alt-Svc header, return.
   - error or expired deadline → `record_h3_failure`, retry over TCP with the cloned request.

Only a genuine mid-flight drop skips steps 4–5, leaving the guard armed.

Deadline expiry takes the fallback branch directly rather than synthesising a `reqwest_middleware::Error`, which would otherwise require adding `anyhow` as a dependency.

The `try_clone()`-returns-`None` path (streaming bodies) skips h3 entirely today and is unchanged. No guard, nothing to demote.

### Public config

Two additions to `AgentHttp3Options`:

```rust
/// Consecutive cancelled HTTP/3 attempts (within a 60s window) that demote an
/// origin to TCP. Default: 3. Set to 0 to disable.
pub upgrade_cancel_strikes: Option<u32>,

/// How long to wait for HTTP/3 response headers before giving up on the h3
/// attempt and retrying over TCP, in milliseconds. Default: 60000. 0 disables.
pub upgrade_attempt_timeout: Option<u32>,
```

`upgrade_attempt_timeout` is in **milliseconds**, unlike its `upgrade*` neighbours which are in seconds, to match `timeout.{connect,read,total}` — fast recovery needs sub-second granularity. The doc comment must call this out explicitly.

Both need doc comments (they generate `index.d.ts`), a regenerated `index.d.ts`, and README entries near line 690. The four existing TTL knobs share one lumped paragraph; `upgradeCancelStrikes` gets its own, as its semantics are not self-evident.

## Error handling

- A cancelled request still rejects with `Aborted`.
- Demotion never retroactively rescues the request that triggered it. It changes what the next request does.
- Deadline expiry is invisible to callers: it falls back and returns a normal response, just slower.
- If the TCP fallback also fails, its error propagates as today.

## Testing

Rust unit tests extending `mod tests` in `alt_svc.rs`:

- strikes below the threshold do not demote; reaching it does (`should_use_h3` returns `None`, origin is in `failed`)
- `confirm_h3` resets the count: 2 strikes → h3 success → 2 strikes does not demote
- `cancel_strikes: 0` disables demotion
- strikes expire: with a short constructed window, 2 strikes → wait → 2 strikes does not demote

JS tests on the harness added in PR #23:

- `test/http3-abort-fallback.test.js` flips from failing to passing. This is the acceptance criterion.
- a new test for the deadline: a low `upgradeAttemptTimeout` (1500ms, leaving headroom for the warm-up QUIC handshake on a loaded CI runner) with `upgradeCancelStrikes: 0`, blackhole UDP, assert the **first** request already falls back with no strikes involved. The caller in that request must have no `timeout` and no `AbortSignal`: either would trigger the pre-existing `Err -> fallback` path by itself and pass whether or not `upgradeAttemptTimeout` exists, so the test also asserts elapsed time is well under quinn's ~30s idle timeout, which is what actually distinguishes the new mechanism from that pre-existing one.

Covering the two mechanisms independently matters: otherwise a bug in one could be masked by the other.

Regression check: the full JS suite plus `cargo test`.

## Acceptance criteria

1. With UDP blackholed and a caller cancelling via `AbortSignal`, a retry loop falls back to TCP within `upgrade_cancel_strikes + 1` attempts.
2. An origin whose h3 works normally is never demoted by interleaved caller aborts.
3. A low `upgradeAttemptTimeout` causes fallback on the first attempt well within quinn's ~30s idle timeout, with `upgradeCancelStrikes: 0` proving strikes played no part and a patient caller (no `timeout`, no `signal`) proving the pre-existing caller-timeout fallback path played no part either.
4. Default behaviour for healthy origins is unchanged: h3 is still preferred and confirmed.
