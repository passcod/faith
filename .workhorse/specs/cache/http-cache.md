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
A stale match under `default` triggers a conditional request; a `304 Not Modified` serves the stored body and refreshes the entry, and a response that permits serving stale can shortcut or survive that revalidation (see [Serving stale entries](#serving-stale-entries)).
The additional mode `ignore-rules` treats any 200 response as cacheable regardless of response caching headers, and serves any stored match regardless of staleness; on a miss it performs a normal request and stores the response.
This exists for callers who want a blunt response cache over an API that sends no caching headers.
A cache mode set on the request wins over the agent's `cache.mode`.

## Serving stale entries

A stored response can be served past its freshness lifetime when the origin has marked it safe to, following the `stale-while-revalidate` and `stale-if-error` response directives (RFC 5861).
Both apply only under `default` mode, the one mode that consults staleness and would otherwise go to the network for a stale match; the other modes either serve a stored match regardless of staleness or bypass the stored entry entirely, so neither directive changes what they do.
A directive takes effect only when the stored response carries it, so serving stale is something an origin opts into per resource.

`cache.serveStale` governs whether the agent honours that opt-in, defaulting to `true` so a compliant origin's directives work without configuration.
Set to `false`, both directives are ignored: a stale match is revalidated in the foreground and a failed revalidation surfaces to the caller, which is the behaviour of an agent that never serves a body it knows to be out of date.
This is one switch over both directives rather than one each, because the reason to refuse stale data (a caller that must not act on it) applies whether the staleness is covered for speed or for an outage.
It applies to the agent's own cache only and does not add request `Cache-Control` directives, so it says nothing to the origin or to any cache between them.

### stale-while-revalidate

A stale match whose age is within the response's `stale-while-revalidate` window is served immediately as a cache hit, and a revalidation of that entry runs in the background rather than blocking the caller.
The caller gets the stale body at cache-hit speed and never waits on the network for it.
The background revalidation refreshes the stored entry when it finishes: a `304 Not Modified` refreshes the entry's freshness, a `200` replaces it, and a failure leaves the existing entry in place.

Background revalidation is single-flighted per cache identity: while one is in flight for a stored response, further requests that match the same identity and land on the stale entry serve it without starting a second revalidation.
Cache identity here is the same method, URL, and `Vary` match that governs [what a stored response answers](#what-a-stored-response-answers).

The background revalidation outlives the foreground request that triggered it: the caller's response settles as soon as the stale body is served, and the revalidation continues on the runtime afterwards.
It carries the agent's configuration like any request and is bounded by the agent's own connect and request timeouts rather than the foreground request's signal, which has already done its job (see [CANCEL](../fetch/cancellation-and-timeouts.md)).
Its outcome belongs to the cache rather than the caller: a network failure, a `5xx`, or an agent closed while it is in flight all pass without surfacing to the caller, as an advisory warm-up's failure does (see [WARM](../agent/warm-up.md)).
Being a real network exchange, it reaches the origin and counts as origin contact for HTTP/3 knowledge, unlike the stale cache hit it accompanies (see [H3UP](../http3/upgrade.md)).
The stale hit the caller receives counts as a response served from the cache like any other; the background revalidation stays out of the caller-request counters and moves `backgroundRequests` instead, since it is a request the agent made rather than one the caller asked for (see [OBS](../agent/observability.md)).

### stale-if-error

A stale match outside any `stale-while-revalidate` window is revalidated in the foreground as `default` mode requires, but a revalidation that fails does not always surface the failure.
When the stored response carries `stale-if-error` and the entry's age is within that window, a revalidation that fails with a network error or a qualifying `5xx` status serves the stored entry instead, so a resource stays available across a transient origin outage.
The qualifying failures are transport and timeout errors and the `500`, `502`, `503`, and `504` statuses; any other response, including the `5xx` statuses the directive does not name, is returned as it came.
Serving the stale entry this way leaves it in the store unchanged, so a later request within the same window can fall back again.

## Interaction with the protocol upgrade layer

A response served from the cache is a cache hit rather than a network exchange, and what that means for HTTP/3 origin knowledge is specified in [H3UP](../http3/upgrade.md).
