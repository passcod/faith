---
id: CACHE
---

# HTTP cache

Each agent can carry an HTTP cache that stores responses and serves or revalidates them according to standard HTTP caching semantics.
The cache is configured at agent construction and disabled unless a store is chosen.

## Stores

Two stores are available: `memory` (in-process) and `disk` (persistent at a caller-supplied path).
The memory store holds a bounded number of entries (configurable via `cache.capacity`); when full, older entries are evicted rather than the cache failing.
The disk store requires `cache.path` and stores cache data at that writable path, so a cache survives process restarts.
With no `cache.store` configured, requests neither consult nor populate any cache.

## Freshness perspective

`cache.shared` selects whose cache this is: a shared cache (respects `s-maxage`, refuses `private` responses) or a private single-user cache (`private` is cacheable, `s-maxage` ignored).
Shared is the default, making the safe choice for proxies and multi-user services automatic.

## Cache modes

The per-request `cache` option and the agent-level `cache.mode` default accept the standard fetch modes with standard semantics: `default`, `no-store`, `reload`, `no-cache`, `force-cache`, and `only-if-cached`.
`only-if-cached` with no matching stored response produces a network error rather than a request.
A stale match under `default` triggers a conditional request (see the conditional GET dimension in [CONF](../quality/conformance.md)); a `304 Not Modified` serves the stored body and refreshes the entry.
The additional mode `ignore-rules` treats any 200 response as cacheable regardless of response caching headers, and serves any stored match regardless of staleness; on a miss it performs a normal request and stores the response.
This exists for callers who want a blunt response cache over an API that sends no caching headers.
A cache mode set on the request wins over the agent's `cache.mode`.

## Interaction with the protocol upgrade layer

A response served from the cache is a cache hit, not a network exchange: it does not update Alt-Svc knowledge, and cannot confirm or deny an origin's HTTP/3 support (see [H3UP](../http3/upgrade.md)).
