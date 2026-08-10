# Eager HTTP/3 probing: happy eyeballs for the Alt-Svc upgrade

Date: 2026-08-10
Issue: follows on from #23's fix; interacts with #27; related to #24 (advertised port)

## Problem

The Alt-Svc upgrade sacrifices a real request to find out whether HTTP/3 actually works.

Today, an advertisement moves an origin into `advertised`, and the *next foreground request*
to that origin is upgraded to HTTP/3 inline, with a TCP retry from a clone if the attempt
fails. When the UDP path is broken — blocked at the client, the server, or anywhere in
between, silently discarding datagrams — that attempt does not fail fast. Nothing errors:
QUIC just retransmits Initials into the void until quinn's idle timeout ends the handshake
(`maxIdleTimeout`, default 30s), or `upgradeAttemptTimeout` (default 60s) gives up on the
attempt, whichever comes first. Only then does the clone go out over TCP.

So the caller sees one request stall for ~30 seconds, on a connection where every other
request completes in milliseconds. Worse, it recurs: the failure parks the origin in
`failed` for `upgradeFailedTtl` (300s), the entry expires, the next response re-advertises,
and another arbitrarily-chosen foreground request is sacrificed. An origin with permanently
broken UDP costs one 30-second stall every five minutes, forever, distributed at random
across whatever the application happens to be doing.

The server is not lying maliciously; this is ordinary reality. Alt-Svc says "I listen on
UDP :443"; it cannot say "and there is UDP connectivity between us", because the server
cannot know that. Only an actual attempt from this client, on this network, can.

## The concept, and what "happy eyeballs" can mean here

Happy Eyeballs (RFC 8305) races *connection attempts* — IPv6 against IPv4, staggered — and
sends the request exactly once, on whichever connection wins. The request is never the
thing at risk; only redundant connection work is.

That exact shape is not available to us. reqwest owns connection establishment and does
not expose it: a request marked `Version::HTTP_3` routes to the h3 pool, anything else to
the TCP pool, and there is no API to race the two pools for a single request (the same
architectural wall as reqwest#1138). Racing at the *request* level instead — send the
request over both, first response wins — is rejected outright below.

What we can do is race at **origin granularity**: keep every foreground request on the
proven TCP path while a background probe attempts QUIC, and flip the origin over only once
the probe has succeeded. This is essentially Chrome's model for QUIC adoption — it races
connections, not requests, and remembers "QUIC is broken" per origin — adapted to the
constraint that reqwest owns the connections and we own only the requests.

The user-visible contract becomes: **no foreground request ever waits on an unverified
HTTP/3 path.** Throughput stays on TCP until HTTP/3 is a proven improvement, then the
floodgates open.

## Goals

- A broken UDP path costs zero foreground latency: no request ever stalls on an
  unconfirmed HTTP/3 attempt, at any point in the origin's lifecycle.
- Healthy origins still adopt HTTP/3 promptly — within about a probe round-trip of the
  advertisement, so typically by the next request.
- Confirmation is not wasted work: the probe's QUIC connection is the one foreground
  requests then use.
- The confirmed-then-breaks path (#23) keeps all its existing protections unchanged.

## Non-goals

- True per-request connection racing. That needs reqwest to expose connection
  establishment or grow an h3-with-TCP-fallback mode; noted as an upstream wish, not
  designed for here.
- Fixing the MTU-blackhole caveat from the application layer. It turns out to be
  largely handled below us already — see "Padding the probe" for why, and for what a
  padded probe would and would not buy.
- Changing how the advertised port is honoured (#24). The probe follows the same
  `port_actionable` rules as today.

## Approach

### 1. `confirmed` becomes the only actionable state

`should_use_h3` stops returning `advertised` entries. An advertisement is evidence worth
*probing*, not worth *routing on*. The cache keeps its existing states — `advertised`,
`confirmed`, `failed`, plus a new single-flight `probing` marker — but only `confirmed`
upgrades a foreground request.

### 2. The probe is a real HTTP/3 request on the shared client

Not a raw QUIC handshake: an actual `HEAD https://host:port/` sent with
`Version::HTTP_3` and its own timeout, whose response is discarded.

The prober holds a clone of the **raw `reqwest::Client`** — available at construction,
where the agent builds the raw client and wraps a clone in middleware
(`src/agent.rs:805`). Sending through the raw client rather than the middleware stack is
load-bearing three times over:

- It bypasses the HTTP cache, so a replayed cached response (which `http-cache` rebuilds
  with the stored HTTP version) can never fake a confirmation — the same hazard the
  middleware registration order already defends against.
- It bypasses `AltSvcMiddleware`, so the probe cannot recurse into probing.
- It shares the h3 connection pool with foreground requests, so a successful probe leaves
  behind a **warm QUIC connection** that the next foreground request rides. The probe is
  also the prewarm; confirmation costs no second handshake.

The probe still inherits everything origin-affecting from the client: default headers,
cookie jar, TLS identity and extra roots, DNS overrides, local address, congestion
control. It tests the path the real requests will use.

**Any HTTP/3 response confirms, regardless of status.** A 401 or 405 to `HEAD /` proves
the transport end-to-end just as well as a 200 — the request semantics are irrelevant, and
`HEAD` is safe, idempotent, and body-less by definition. The one check kept from the
inline path: the response's version must actually be HTTP/3, else it counts as a failure.

A completed QUIC handshake is a stronger signal than it may look: RFC 9000 §14.1 makes
the client expand every datagram carrying an Initial to at least 1200 bytes, and the
server likewise for ack-eliciting Initials — with the certificate flight several
full-size datagrams on top. Success therefore proves the path carries 1200-byte
datagrams in **both** directions. What happens above 1200 is not the probe's problem,
or anything the application layer can influence — see "Padding the probe" below.

### 3. Triggering and single-flight

A probe is kicked off, if none is in flight for the origin:

- when `record_alt_svc` inserts an actionable advertisement — fastest adoption, since the
  probe races the gap before the caller's next request; and
- when `should_use_h3` touches an `advertised` entry — the belt to the above's braces,
  covering entries that outlived a completed-but-unconfirmed probe window.

Both call sites are inside request handling, so a tokio runtime is present to spawn onto.
Single-flight is a `probing` moka cache keyed by origin with TTL = probe timeout plus
slack; `entry().or_insert_with` gives the atomic check-and-claim, and expiry doubles as
crash recovery if a probe task dies without reporting.

### 4. Outcomes

- **HTTP/3 response** → `confirm_h3(origin, port)`. Identical promotion to today's, just
  triggered by the probe instead of a sacrificed foreground request.
- **Error, timeout, or non-h3 response** → `record_h3_failure(origin)`. The 300s `failed`
  cooldown and re-advertisement cadence are unchanged — but the retry cycle now costs a
  background HEAD every five minutes instead of a stalled user request.

### 5. Hints seed `confirmed` directly

A hint is the *user's* assertion, not the server's, and today it makes the very first
request speak HTTP/3 — which is also the only way an h3-only origin (no TCP listener) can
work at all. Routing hints through the probe would break that and second-guess an explicit
instruction. So hints skip `advertised` and seed `confirmed` with their current forever
expiry. Distrust is reserved for what servers advertise; failure demotes a hinted origin
exactly as it does now.

### 6. What stays

Everything guarding *confirmed-then-breaks* is untouched: the inline clone-fallback on h3
attempts, `upgradeAttemptTimeout`, cancellation strikes and the `H3AttemptGuard`. A
confirmed origin whose UDP path dies mid-life still demotes through those mechanisms.

The probe does soften #27's worst case, though: after any demotion — including a
false-positive one from a concurrent abort burst — re-entry to HTTP/3 happens via a
background probe, never a foreground stall. Wrongful demotion now costs 300s of TCP and
nothing else. (#27's minimum-in-flight-duration hardening remains worth doing.)

### 7. Options

- `upgradeProbe: boolean`, default `true`. `false` restores today's inline upgrade, for
  operators who cannot tolerate synthetic requests: per-request billing, WAFs that
  ratelimit odd traffic, or audit logs where an unexplained `HEAD /` raises questions.
  `upgradeEnabled: false` remains the master switch and implies no probing.
- `upgradeProbeTimeout: u32` milliseconds, default 5000. This bounds background work
  only, so it can afford to be generous relative to foreground budgets; a healthy
  handshake plus HEAD completes in 1–2 RTTs, so 5s covers even very slow paths while
  still failing an origin ten times faster than the status quo stalls a request.

### 8. Lifecycle

Probe tasks are held as abort handles on the agent (alongside the existing background
resources) and aborted in `close()`. Without that, a probe's raw-`Client` clone would keep
the connection pool alive for up to the probe timeout after close. Aborting mid-handshake
just abandons the quinn connection attempt, which is safe.

### 9. Padding the probe: considered, deferred

Could the probe also exercise the upload direction — a body on the HEAD, or a deliberately
oversized header block — to shrink the MTU caveat further? Both are mechanically
expressible, and the header variant is even sound; but the MTU rationale dissolves on
inspection of what quinn actually does with datagram sizes.

**Application data cannot force datagram sizes.** reqwest builds its QUIC endpoint from
`TransportConfig::default()` (reqwest `async_impl/client.rs`), which means quinn starts
every connection at `initial_mtu` = 1200 and raises it only through DPLPMTUD: dedicated
PING+PADDING probe packets, re-run on a 600s interval, upper bound 1452, and the MTU
moves only after a probe of that exact size is acknowledged. Probe loss is not treated as
congestion loss, and an active black-hole detector drops the MTU back (60s cooldown) if
full-size packets start vanishing mid-life. Two consequences:

- On a fresh connection — which the probe always is — a bulk upload is packed into
  1200-byte datagrams, the size the handshake *just proved in both directions*. The
  padding transits fine and demonstrates nothing new about MTU.
- The >1200 blackhole the caveat worries about is already handled below us, gracefully:
  DPLPMTUD never raises the MTU across it, and if a path narrows later, the black-hole
  detector walks back to 1200 rather than stalling. The truly pathological residue — a
  path that passed 1200-byte handshake datagrams and later drops even those — is a
  *confirmed-then-breaks* event, which is exactly what the #23 machinery (attempt
  timeout, strikes, clone fallback) exists for. No probe-time padding reaches it.

What padding *would* test is something else: sustained-flow treatment. Middleboxes and
policers exist that admit a QUIC handshake but throttle, reclassify, or kill UDP flows
that persist beyond a few packets — and the handshake is thinnest as evidence in the
upload direction, where the client sends the least. If that failure family shows up in
practice, the right shape is a **padding header, not a body**: reqwest's h3 path does
send an attached body regardless of method (verified in `h3_client/pool.rs` — no method
check), but it spawns the upload concurrently with awaiting the response, so a server
that responds early and issues `STOP_SENDING` cuts the padding short at an unknowable
point, and the probe's confirm-on-any-response logic would need to learn to ignore
body-send errors. A junk header, by contrast, *must* be read in full before the server
can respond at all, so the bytes are on the wire before any confirmation — and even a
`431 Request Header Fields Too Large` still confirms HTTP/3. Sized around 4KB it stays
under the common 8–16KB header caps (the conformance matrix's oversized-headers rows are
prior art on tolerance), and it wants high-entropy content, since QPACK Huffman-encodes
literals and would shrink repetitive padding.

Deferred, not designed in: an `upgradeProbePadding` byte-count option (default 0) slots
into the probe construction trivially if the sustained-flow case materialises. The
first cut keeps the probe minimal.

## Timing, before and after

| Scenario | Today | With probing |
| --- | --- | --- |
| Healthy origin, request after advertisement | upgraded inline; ~0 added | TCP; probe confirms in ~1–2 RTT |
| Healthy origin, steady state | HTTP/3 | HTTP/3, first h3 request on a prewarmed connection |
| Broken UDP, request after advertisement | **stalls 30–60s**, then TCP | TCP, unaffected |
| Broken UDP, every `failed` expiry | another 30–60s stall | one background HEAD |
| h3-only origin via hint | first request h3 | unchanged (hints seed `confirmed`) |

The adoption delay on healthy origins is real but tiny: the origin serves TCP for about
one probe round-trip longer than today. The conformance `h3-upgrade` dimension (three
warmup requests before asserting HTTP/3) should still pass on localhost, and is the canary
for this trade-off.

## Testing

On the existing `h3-blackhole` fixture (TCP proxy always healthy, UDP relay switchable):

- Blackholed from the start: advertisement received, then every foreground request
  completes within a small latency budget (a few RTTs, not tens of seconds), and the
  origin lands in `failed`. This is the headline property and fails against today's code.
- Healthy: existing `h3-upgrade` conformance dimension keeps passing as-is.
- Single-flight: a concurrent burst of requests right after the advertisement produces
  one probe (observable via the relay's datagram counters).
- Recovery: blackhole → `failed`; restore the relay, expire `failed` (short TTL in test),
  re-advertise → probe → subsequent requests on HTTP/3.
- `close()` with a probe in flight against a blackholed origin returns promptly.
- Unit: `should_use_h3` no longer returns advertised entries; hints land in `confirmed`;
  probing marker dedupes and expires. (#33's property tests would cover the extended
  state machine.)

## Follow-on: path-time-aware preference

Once origins carry per-protocol state anyway, the same cache can answer a better question
than "does HTTP/3 work?" — namely "is HTTP/3 *worth it* here?". A UDP path can be intact
but slower than the TCP one: a QUIC-hostile middlebox, an anycast split routing UDP
somewhere worse, a server whose h3 stack is simply less tuned. Today (and under this
design) a working-but-slow h3 path is confirmed and preferred anyway.

Implemented alongside the probe (`upgradeSlowFactor`, default 2.5, 0 disables;
`upgradeSlowTtl`, default 600s), to the following shape:

- **Measure.** Keep an exponentially-weighted moving average of time-to-response-headers
  per origin *per protocol family* (TCP: h1/h2 together; QUIC: h3). Foreground requests
  feed the TCP average for free; h3 requests (probe included) feed the QUIC one. EWMA
  rather than a window: two `f64`s per origin, no sample storage, and it naturally decays
  stale history.
- **Compare like with like.** Time-to-headers includes server think-time, which varies
  per endpoint far more than per transport; only the *averages across many requests* are
  comparable, never individual samples. First-flight samples on either side include a
  handshake and should either be excluded or averaged in knowingly. A minimum sample
  count per side gates any decision.
- **Decide with a handicap, not a tie-break.** The preference order is not symmetric,
  and deliberately so:
  - h1 vs h2: never act on timings. Multiplexing and header compression outweigh small
    path-time differences, and reqwest negotiates this via ALPN anyway.
  - h3 vs h2: prefer h3 at parity *and* when moderately slower, because its advantages
    (no head-of-line blocking, connection migration, faster resumption) pay off beyond
    the mean. Only a large sustained gap — h3's EWMA worse than TCP's by a factor of
    2–3× and by an absolute floor (a few ms, so LAN-fast origins don't flap on noise) —
    demotes the origin to TCP.
- **Demote softly, re-try cheaply.** A latency demotion is a new cache state
  (`slow`, say), distinct from `failed`: the path *works*, so the origin must not be
  treated as broken, and re-advertisements should not re-arm inline upgrades. On the
  demotion's TTL expiry, re-entry goes through the same background probe as everything
  else — which by then is the established, zero-foreground-cost way to ask "has this
  path improved?". The probe machinery is what makes periodic re-evaluation affordable;
  without it, every re-try would again gamble a foreground request.

The reason to build the probe first is that it creates both halves of what this needs:
h3 timing samples that cost the foreground nothing, and a safe re-entry path for any
demotion policy layered on top.

## Rejected alternatives

**Per-request racing (literal happy eyeballs).** Send the request over both h3 and TCP,
first response wins. Duplicates execution — fatal for non-idempotent requests, and even
for GET it doubles origin load, double-triggers side effects and billing, and interleaves
cookie mutations. Browsers do not race requests either; they race connections. Racing
*connections* per request is the right long-term shape but requires upstream reqwest work
(expose h3 pool prewarming, or an h3-with-fallback race mode); worth filing upstream, not
worth forking over.

**Probing with quinn directly.** A handshake-only probe without an HTTP request. Rejected
because reqwest does not expose its quinn `Endpoint`, so this means a second endpoint with
hand-duplicated TLS roots, identity, ALPN, congestion and socket config that must be kept
in lock-step with reqwest's — and the probed connection can never join reqwest's pool, so
the first real h3 request pays a full second handshake. It also proves less: ALPN
acceptance, but nothing about the h3 layer. All cost, no benefit over an in-band HEAD.

**Shadowing the triggering request.** Duplicate the foreground request over h3 in
parallel, serve the TCP response, use the h3 result only as a signal. Avoids the synthetic
request but inherits the duplicate-execution problem for anything non-safe, doubles
auth-bearing traffic, and ties the probe's lifetime to a foreground request that may be
aborted for unrelated reasons (feeding back into #27). `HEAD /` is strictly simpler.

**Shrinking `upgradeAttemptTimeout` instead.** A 2–3s deadline caps the stall but still
sacrifices one foreground request per failure cycle, adds double-send risk for
non-idempotent requests (documented on that option), and mispunishes slow-but-working
paths. It treats the symptom; the probe removes the exposure.
