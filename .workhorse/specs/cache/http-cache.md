---
id: CACHE
---

# HTTP cache

Each agent can carry an HTTP cache that stores responses and serves or revalidates them according to standard HTTP caching semantics.
The cache is configured at agent construction and disabled unless a store is chosen.

## Stores

Two stores are available: `memory` (in-process) and `disk` (persistent at a caller-supplied path).
The memory store holds a bounded number of entries, `cache.capacity`, defaulting to 10,000; when full, older entries are evicted rather than the cache failing.
The disk store requires `cache.path` and stores cache data at that writable path, so a cache survives process restarts.
A disk store whose path cannot be created or written fails the requests that reach it with a network error, rather than falling back to an uncached request.
With no `cache.store` configured, requests neither consult nor populate any cache.

## What a stored response answers

A stored response is identified by the request method and the full URL, and each identity holds one stored response.
A response whose `Vary` header names request headers is served only to a request whose values for those headers match the ones it was stored against; a request that does not match goes to the network and its response replaces the stored one.
A response is stored as it came off the wire, its content coding intact, and decoded on the way out to whichever request it answers (see [ENC](../fetch/content-encoding.md)).

## Freshness perspective

`cache.shared` selects whose cache this is: a shared cache (respects `s-maxage`, refuses `private` responses) or a private single-user cache (`private` is cacheable, `s-maxage` ignored).
Shared is the default, making the safe choice for proxies and multi-user services automatic.

## Cache modes

The per-request `cache` option and the agent-level `cache.mode` default accept the standard fetch modes with standard semantics: `default`, `no-store`, `reload`, `no-cache`, `force-cache`, and `only-if-cached`.
`only-if-cached` with no matching stored response produces a network error rather than a request.
A stale match under `default` triggers a conditional request; a `304 Not Modified` serves the stored body and refreshes the entry.
The additional mode `ignore-rules` treats any 200 response as cacheable regardless of response caching headers, and serves any stored match regardless of staleness; on a miss it performs a normal request and stores the response.
This exists for callers who want a blunt response cache over an API that sends no caching headers.
A cache mode set on the request wins over the agent's `cache.mode`.

## Interaction with the protocol upgrade layer

A response served from the cache is a cache hit rather than a network exchange, and what that means for HTTP/3 origin knowledge is specified in [H3UP](../http3/upgrade.md).
