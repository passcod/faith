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

## Threading

Response work (body reads, trailer waits) runs on Fáith's own async runtime, not the libuv worker pool: concurrency is not bounded by `UV_THREADPOOL_SIZE`, and a saturated worker pool does not stall in-flight responses.
