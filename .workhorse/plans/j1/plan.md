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
helper with `dns.servers`. Worth its own breakdown entry if it wants to ship
separately, since it is a bench-validity bug independent of stale serving.

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

## Finding: empty AAAA answers are not cached, so A-only names never hit the cache

Measured with the helper (A-only zone, TTL 60, 250ms answer delay), hickory
re-queries AAAA on **every** lookup and each lookup pays a full round trip:

| zone | lookup 1 | lookup 2 | queries |
|------|----------|----------|---------|
| A + AAAA, TTL 60 | 251ms | 0ms | `A, AAAA` |
| A only, TTL 60 | 251ms | 251ms | `A, AAAA, AAAA, AAAA` |

The A record caches fine; the no-records AAAA answer does not, because
`negative_min_ttl` defaults to 0 (`ResolverOpts`, hickory `src/config.rs`), so the
NODATA entry expires the instant it lands. Under `Ipv4AndIpv6` every lookup then
blocks on that AAAA query.

Setting `negative_min_ttl` to 30s makes lookup 2 instant (0ms) and drops the
repeat AAAA queries entirely — confirmed by measurement.

This matters for the card three ways:

- Much of the slow-resolver pain this card targets is this gap, not TTL expiry.
  A one-line resolver-option change wins a large part of it with no wrapper at all.
- A stale-serving wrapper keyed only on positive answers would still block on the
  uncached AAAA query, so it would deliver **nothing** for A-only names — a very
  common case. The wrapper has to cover NODATA answers too, or be paired with a
  non-zero `negative_min_ttl`.
- The bench needs both rows to be honest about where the win comes from: the
  negative-caching fix and stale-serving are separate effects.
