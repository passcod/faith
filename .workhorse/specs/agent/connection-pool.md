---
id: POOL
---

# Connection pool

Each agent pools connections so subsequent requests to the same endpoint skip DNS, TCP, and TLS setup.
Pooling is always on; the options bound how long and how many idle connections are kept.

`pool.idleTimeout` closes a connection after that many seconds of inactivity.
It defaults to 90 seconds, and the same window bounds how long an idle connection appears in `connections()` (see [OBS](observability.md)).
`pool.maxIdlePerHost` caps idle connections kept per origin: scheme, host, and port together, so `https://example.com`, `https://example.com:8443`, and `http://example.com` are capped separately despite the option's name.
Once an origin sits at the cap, a connection that would otherwise return to the pool is closed instead, so the connections already idle are the ones that survive.
The default is no limit.
HTTP/1 connections return to the pool once their response body has been fully read or discarded; an unconsumed body holds its connection (see [BODY](../response/reading-the-body.md)).
HTTP/2 and HTTP/3 connections multiplex, so reuse does not depend on body consumption.
The connection established by a successful HTTP/3 probe lands in the same pool, so the first upgraded request starts on a warm connection (see [PROBE](../http3/probing.md)).
A connection opened by `preconnect(origin)` lands in the pool the same way, so the first request to that origin starts warm (see [WARM](warm-up.md)).

## Reusing a connection that has died

An origin may close a pooled connection without signalling it in the response, and the close can land after the connection has already been returned to the pool.
A request written into such a connection never reaches the origin, so Faith sends it again on another connection rather than failing the caller.

A request is sent again only when the connection ended before any part of a response arrived.
A refused connection, a failed handshake, a timeout, and any response at all are answers about the origin, and are returned to the caller as they are.
A connect failure is answered first by re-resolving the name when the address came from an expired DNS entry, that address having been assumed rather than confirmed; the failure reaches the caller once the fresh address has failed too (see [DNS](dns.md)).
An origin that closes idle connections closes all of them, so the pool can hold several that are already gone and a fresh attempt can draw another one; a request is therefore sent up to five further times before the failure reaches the caller.

Only requests that can be sent again without changing what the origin has done are sent again.
That means the method is one of `GET`, `HEAD`, `OPTIONS`, `TRACE`, `PUT`, or `DELETE`, and the body is one that can be produced a second time.
`POST` and `PATCH` are never sent again: a connection that died carries no evidence of whether the origin processed the request before it went, so a request whose repetition would count twice surfaces the failure instead.
A request with a `ReadableStream` body is never sent again either, the stream having already been consumed (see [REQ](../fetch/request.md)).
