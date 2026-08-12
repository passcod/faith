---
id: RESP
---

# Response

`fetch()` resolves to Fáith's own `Response` class.
It is not constructible by callers; it mirrors the Web API `Response` surface and adds server-side information a browser response cannot carry.
`webResponse()` converts to a genuine Web `Response` when an API demands one (see [BODY](reading-the-body.md)).

## Standard properties

`status`, `ok` (status in 200-299), `url` (final URL after redirects), and `redirected` behave as in the fetch standard (see [REDIR](../fetch/redirects.md) for the `redirected` port caveat).
`headers` is a Web API `Headers` object, built lazily and memoised; Fáith ships no custom `Headers` class.
A header whose value is not valid UTF-8 is dropped rather than surfaced in a lossy form.
`statusText` carries the canonical reason phrase for the status code.
HTTP/2 and HTTP/3 have no reason phrases on the wire, so the phrase is simulated from well-known codes there; unknown codes yield an empty string.
`type` is always `basic`.

## Fáith-specific properties

`version` is the HTTP version of the response, the final one after any redirects and protocol upgrades (e.g. `HTTP/1.1`, `HTTP/2.0`, `HTTP/3.0`).
`peer` describes the remote peer: `address` (IP and port, when available) and `certificate` (the DER-encoded leaf certificate when the connection was TLS, as a Buffer).
Each access builds a fresh object and a fresh Buffer copy.
`trailers` is a promise of the trailing headers (see [TRL](trailers.md)).
`timing` is a per-request timing breakdown in the shape of a `PerformanceResourceTiming` (see [Request timing](#request-timing)).

## Request timing

`timing` reports how long the request took, phase by phase, in the shape of a Web `PerformanceResourceTiming`.
Fields that map onto that interface take its names; those with no equivalent take a descriptive name.
Each access builds a fresh object reflecting what is known at that moment.

The breakdown covers the phases observable at Fáith's request boundary:

- `fetchStart` — the moment the request began, and the origin all other phases are measured against
- `responseStart` — the first byte of the response, i.e. the response headers, has arrived
- `responseEnd` — the last byte of the body has arrived, reading `null` until the body is finished (fully read or discarded, see [BODY](reading-the-body.md))
- `reused` — whether the request travelled on a pooled connection rather than a freshly established one (see [POOL](../agent/connection-pool.md))
- `nextHopProtocol` — the protocol the request travelled over, as an ALPN Protocol ID (RFC 7301): `h3`, `h2`, `h2c`, `http/1.1`, and so on, whether or not the connection actually negotiated over ALPN

Phase timestamps are fractional milliseconds on a monotonic clock shared across the breakdown, so consumers subtract one phase from another to obtain a duration; `fetchStart` is the earliest and the rest are no earlier than it.
The breakdown starts at the request boundary, so the interval from `fetchStart` to `responseStart` covers connection acquisition (pool wait, or DNS, connect, and TLS handshake on a fresh connection) and the server's own turnaround as a single span rather than as separate phases.

`responseStart` less `fetchStart` is the time to response headers.
That measurement is taken at a single instrumentation point and is the same one the per-protocol path-time average consumes for HTTP/3 slow-path demotion (see [PROBE](../http3/probing.md)), so a request's surfaced timing and the average it feeds never diverge.

## Threading

Response work (body reads, trailer waits) runs on Fáith's own async runtime, not the libuv worker pool: concurrency is not bounded by `UV_THREADPOOL_SIZE`, and a saturated worker pool does not stall in-flight responses.
