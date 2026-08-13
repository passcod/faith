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
- `prefetch` and `clear_cache` (spec:WARM, spec:NETCHG) must clear/participate in
  the wrapper map too, not just hickory's cache.
- `networkChanged` clearing must drop stale entries so a network move doesn't keep
  serving old IPs.

## Benchmark

Card requires a bench row before ship: the win only shows against slow resolvers.
The `features` suite (`bench/run.mjs`, `runFeatures`) already has a DNS group
(`dns:hickory` vs `dns:system`, cold mode via `localhost`). Add a stale-serve
variant there. Slow-resolver effect needs a delayed/slow DNS answer, not just a
delayed HTTP response (`--delay` only delays the server) — the harness has no DNS
delay knob today, so that's a gap to close for a meaningful row.
