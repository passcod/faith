# Network-change signal API

Implements `agent.networkChanged()` per [NETCHG](../../specs/agent/network-change.md).

## Technical notes

**The pool is the hard part.** `reqwest::Client` exposes no way to drop pooled connections: there is no `clear_pool`, and hyper's idle pool is private (verified against reqwest 0.13.4's `impl Client`, which carries only the request-building methods). The only way to drop the pool is to drop the client, so `networkChanged()` rebuilds it.

That rebuild is what makes the in-flight guarantee fall out for free. `FaithOptions::extract` clones the `Agent` per request (`src/options.rs:200`), so a request in flight holds its own clone of the old `ClientWithMiddleware` and runs to completion on the old pool; the old pool drops when the last such clone finishes. Requests starting after the signal clone the agent afresh and get the new client.

**Rebuilding needs a recipe.** `with_options_inner` interleaves validating options with applying them to a `ClientBuilder`, and consumes `AgentOptions` doing it, so it cannot be re-run. Split it: validate once into a `ClientRecipe` of Rust-native cloneable values (no napi types — PEM inputs become `Identity`/`Certificate`, both `Clone`), then build clients from the recipe as often as needed.

`apply_node_env` reads the environment at build time, but AGENT says env vars are read at construction. Capture its three results (extra CA certs, accept-invalid-certs, no-proxy) into the recipe at construction so a rebuild replays them rather than re-reading a changed environment.

**What must survive the rebuild** has to be shared state the new client is built around, not rebuilt with it: the cookie jar, the HTTP cache manager (a fresh `MokaManager` would silently empty the in-memory cache), the alt-svc cache, the DNS resolver, `stats`, and the connection tracker. All are already `Arc`-shaped or cheap-clone, which is what makes the swap safe against the per-request agent clones.

**Hints have no marker today.** `add_hint` writes into `confirmed` with an expiry ~10,000 hours out, indistinguishable from an observation-confirmed entry. Since the signal demotes observation-confirmed origins but keeps hinted ones, the cache needs to know which is which: keep the hinted origins in their own map and re-seed `confirmed` from it after clearing.

**Reuse `demote_slow`'s shape** for the confirmed → advertised transition. It already moves an entry to `advertised` with a fresh advertised TTL and clears the QUIC average, which is the same transition the signal wants.

## Steps

- [x] Spec: resolve the observability ambiguity the governing rule leaves (does `connections()` clear?), and cover warm-up records
- [x] `FaithResolver::clear_cache()` — sync, no-op when the resolver was never initialised
- [x] `AltSvcCache`: track hinted origins, add `network_changed()` (demote observation-confirmed, clear failed/cancellations/slow/probing/EWMAs, re-seed hints)
- [x] Refactor `with_options_inner` into `ClientRecipe` (validate) + `build` (apply), preserving shared state
- [x] `Agent::network_changed()` napi method: rebuild clients, swap prober, flush DNS, reset alt-svc, clear warm records
  - [x] Guard the warm-up race with a generation counter: a `preconnect` in flight across the signal would otherwise mark its origin warm on the strength of a connection in the dropped pool
- [x] Rust unit tests for the alt-svc reset
- [x] JS tests: pool drop, DNS re-resolution, hints kept, in-flight untouched, closed agent, idempotence
- [x] JS tests that the rebuild keeps configuration: TLS trust, flow-control windows, redirect, timeouts, `localAddress`, both cache stores
- [x] README: `Agent.networkChanged()` under the Agent methods
- [x] Test cases file, plus `cargo test` and the JS suite green

## Notes from the build

**A pre-existing DNS bug surfaced in the refactor and is fixed here.** `dns.overrides` were registered on the client only in the non-system branch, so with `dns.system: true` they took no effect and a malformed override address never threw — both against DNS and AGENT. Splitting validation from application forced the question of where the overrides belong, and reqwest wraps whichever resolver it was given in its override layer, so they now register unconditionally. The existing test passed either way because it overrode `localhost` to localhost's own address; the new test overrides a `.invalid` name the system resolver cannot resolve, and it was confirmed to fail against the old scoping before the fix went in.

**`connections()` is kept rather than cleared**, which the spec now says outright. The signal cannot distinguish a connection it dropped from the pool from one still carrying an in-flight request, so clearing the list would hide live connections exactly when a caller is most likely to be watching them. This drew the governing rule's line at state the agent *decides from* rather than state it *reports*, which puts `stats()` and `connections()` on the same side coherently.
