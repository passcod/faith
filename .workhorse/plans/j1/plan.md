# Serve stale DNS entries while revalidating (J1)

## Feasibility: hickory config vs wrapper cache

Investigated hickory-resolver 0.26.1 (locked version). **hickory cannot be
configured for stale-while-revalidate; a thin wrapper cache is needed.**

What hickory's cache offers (`ResolverOpts`, `src/config.rs`): TTL clamping only
(`positive_min_ttl`, `positive_max_ttl`, negative equivalents), `cache_size`, and
manual flush (`clear_cache`, `clear_lookup_cache`). There is no serve-stale knob.

Why config alone can't do it (`src/cache.rs`):
- `get()` returns `None` once `now > valid_until` (`is_current`), and the entry is
  evicted by moka's `expire_after`. Expired answers are never handed back.
- There is no background-refresh / prefetch-on-expiry path anywhere in
  `caching_client.rs` or `resolver.rs`.
- Raising `positive_min_ttl` only lengthens the TTL — it keeps a stale answer
  without ever revalidating, which is not the same thing and gives no refresh.

### Wrapper design (feasible)

`FaithResolver` (`src/dns.rs`) already owns the `TokioResolver` and implements
`reqwest::dns::Resolve`, so the wrapper lives there:

- Keep a `host -> (addrs, deadline)` map alongside the resolver.
- Fresh hit: return addrs.
- Stale hit: return the stale addrs immediately, and spawn one background
  `lookup_ip` to refresh the entry (single-flight per host).
- Miss: normal blocking lookup, then store.

Building blocks confirmed present in 0.26.1:
- `LookupIp::valid_until() -> Instant` gives the real TTL deadline to store.
- `resolver.clear_lookup_cache(name, record_type)` lets the background refresh
  force a fresh network lookup past hickory's own (possibly expired) cache entry,
  so the two caches don't fight.

Interaction with existing behaviour to preserve:
- `prefetch` (spec:WARM) must participate in the wrapper map, not just hickory's cache.
- Stale entries must not outlive `networkChanged()`.

Revised after rebasing onto H1 (#78), which rewrote `src/dns.rs` to ~800 lines:
the resolver now hangs off a `Generation` behind `built: OnceCell<Arc<Built>>`, and
`networkChanged()` drops the generation rather than flushing a cache. So the stale
map belongs **inside the generation**, which gets the network-change behaviour for
free instead of needing its own clearing path.

## Benchmark

Card requires a bench row before ship: the win only shows against slow resolvers.
The `features` suite (`bench/run.mjs`, `runFeatures`) already has a DNS group
(`dns:hickory` vs `dns:system`, cold mode via `localhost`). Add a stale-serve
variant there. Slow-resolver effect needs a delayed/slow DNS answer, not just a
delayed HTTP response (`--delay` only delays the server) — the harness has no DNS
delay knob today, so that's a gap to close for a meaningful row.

### The existing DNS bench rows measure nothing (found while rebasing onto H1)

H1 made `localhost` an always-exempt name, routed to the system resolver whatever
Faith's own settings say (spec:DNS#exempt-names, `is_exempt` in `src/dns.rs`).
Both DNS rows in `runFeatures` set `urlHost: "localhost"`, so `dns:hickory` and
`dns:system` now resolve through the *same* system resolver and the comparison is
empty. `bench/` was untouched by H1, so this is live on main.

Fixing it is a prerequisite for this card's row, not a separate cleanup: the rows
must use a non-exempt name (the helper's zone uses `.test`) and point Faith at the
helper with `dns.servers`. Spun out to the breakdown, since it is a bench-validity
bug independent of stale serving.

## Test and bench DNS server

`test/lib/dns-server.js` is a controllable authoritative nameserver (UDP + TCP,
ephemeral port) with a settable zone, per-name TTL, settable answer delay, a query
log, mid-run `set()`, and `fail()` for servfail/nxdomain/drop. Its selftest is
`test/dns-server.test.js`, checked against Node's c-ares resolver so a failure
there is the helper's wire format rather than anything about Faith.

`dns.overrides` can't stand in for it: an override resolves without asking a
nameserver, so nothing is cached and no TTL ever expires. The delay knob is what
makes a bench row meaningful, since serving stale only beats resolving fresh when
the resolver is slow.

Verified against a real hickory 0.26.1 resolver pointed at the helper: answers,
TTLs, cache hits, and post-expiry re-queries all behave as expected, including
0x20 case randomisation (the helper echoes the question bytes verbatim).

### Wiring: resolved by H1 (#78)

This was previously a blocker — `new_resolver()` took the system configuration or
fell back to Google, so nothing could make Faith query the helper. H1 landed
`dns.servers`, an ordered list of resolver URLs, so the helper wires up directly:

```js
new Agent({ dns: { servers: [`udp://127.0.0.1:${server.port}`] } })
```

`udp://` is conventional DNS, and a port in the URL overrides the conventional 53,
which is what makes an ephemeral-port helper reachable. `dns.timeout` (5s default)
bounds the lookup, so it needs raising above the helper's `delayMs` in any test that
sets a large delay.

Use a non-exempt name: `localhost` and `.local` are handed to the system resolver
regardless of `dns.servers`, so the helper would never be asked. The helper's zone
uses `.test` names for this reason.

## Retracted: "empty AAAA answers are not cached"

An earlier version of this plan recorded that hickory re-queries AAAA on every
lookup for an A-only name, so such names never hit the cache. **That was an
artefact of the test helper, not Faith or hickory behaviour.**

A negative answer has no record to hang a TTL on, so RFC 2308 carries it in an
authority-section SOA, and `DnsResponse::negative_ttl` (hickory-proto
`src/op/dns_response.rs`) reads it from there. The helper sent no SOA, so hickory
saw no negative TTL, fell back to `negative_min_ttl` (default 0), and declined to
cache — which is exactly what RFC 2308 requires: "Negative responses without SOA
records SHOULD NOT be cached."

With the helper fixed to send an SOA, as a real resolver does, the A-only case
caches correctly (`prefetchDns` against an A-only zone, TTL 60, 250ms delay):

| helper | lookup 1 | lookup 2 | lookup 3 | queries |
|--------|----------|----------|----------|---------|
| SOA present, negative TTL 30 | 263ms | 0ms | 0ms | `A, AAAA` |
| no SOA (negative TTL 0) | 262ms | 252ms | 251ms | `A, AAAA, AAAA, AAAA` |

So there is no negative-caching gap to fix and no `negative_min_ttl` change to
make. The lesson is about the harness: a nameserver that omits the SOA makes every
negative answer look uncacheable, so any measurement taken against one is
measuring the harness. The helper now sends it by default.

## Build steps

- [x] Fix the test nameserver to send an authority-section SOA on negative answers,
      so negative caching behaves as it does against a real resolver
- [x] Add `dns.serveStale` (default true) and `dns.maxStale` (default one hour) to
      `AgentDnsOptions`, threaded into `ResolverSettings`
- [x] Hold stale answers in a bounded cache inside `Generation`, keyed by host, with
      `valid_until` taken from `LookupIp::valid_until()`
- [x] Serve an expired entry inside the window and spawn a single-flighted background
      refresh; a fresh entry falls through to hickory's own cache
- [x] Classify the refresh outcome: resolves replaces, `NoRecordsFound` retires,
      anything else keeps the entry
- [x] Add `StaleAddressRetry` middleware, outside `DeadConnectionRetry`, that
      re-resolves and re-attempts once on a connect failure against a stale address
- [x] Rust unit tests for the stale window, the `serveStale` switch, the retry arming,
      and the network-change drop
- [x] JS tests driving Faith at the helper through `dns.servers`
- [ ] Bench row showing the win against a slow resolver — blocked on card W2, which
      restores the DNS rows and gives the harness a DNS delay knob

## Corrections made to the spec while implementing

- The stale-address retry cannot cover a `ReadableStream` body. `Body::try_clone`
  returns `None` for a streaming body whether or not it has been read (reqwest
  `src/async_impl/body.rs`), so there is no second copy to send. The safety argument
  holds — nothing reached the origin — but feasibility does not, so the spec now
  names streaming bodies as the exception. `POST` and `PATCH` with buffered bodies do
  recover, which is more than the dead-connection replay allows.
- Removed a criterion claiming both address families' outcomes are cached. It was
  written to fix the retracted negative-caching finding above, describes behaviour
  that already worked, and implied this card changed something there.

## Left unspecified, decided by implementation

Two behaviours the spec does not settle. Both are recorded here rather than guessed
at silently, and either could reasonably go the other way.

**`prefetchDns` on a stale entry returns without waiting for the refresh.** It goes
through the same lookup path as a request, so it serves stale and starts the refresh
behind it. That keeps the WARM contract ("a later request skips the lookup") true,
since a later request is served stale and is fast. But WARM also describes the promise
as settling when "the DNS answer landing in the cache" happens, which reads like a
fresh answer, and a caller warming deliberately before a burst might want the fresh
one. The alternative is for `prefetchDns` to await the refresh, which would make an
explicit warm-up slower than a real request.

**A redirect to another host attributes the connect failure to the original host.**
reqwest follows redirects below the middleware, so `req.url().host_str()` is the host
the caller asked for, not the redirect target. A connect failure at the target
therefore invalidates the original host's stale entry and re-attempts the whole
request. The outcome is correct (the failure still surfaces) and one attempt is
wasted. The existing dead-connection layer has the same shape, so this is consistent
rather than novel, but it is imprecise and worth knowing about.
