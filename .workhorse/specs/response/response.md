---
id: RESP
---

# Response

`fetch()` resolves to Faith's own `Response` class.
It is not constructible by callers; it mirrors the Web API `Response` surface and adds server-side information a browser response cannot carry.
`webResponse()` converts to a genuine Web `Response` when an API demands one (see [BODY](reading-the-body.md)).

## Standard properties

`status`, `ok` (status in 200-299), `url` (final URL after redirects), and `redirected` behave as in the fetch standard (see [REDIR](../fetch/redirects.md) for the `redirected` port caveat).
`headers` is a Web API `Headers` object, built lazily and memoised; Faith ships no custom `Headers` class.
A header whose value is not valid UTF-8 is dropped rather than surfaced in a lossy form.
`statusText` carries the canonical reason phrase for the status code.
HTTP/2 and HTTP/3 have no reason phrases on the wire, so the phrase is simulated from well-known codes there; unknown codes yield an empty string.
`type` is always `basic`.

## Faith-specific properties

`version` is the HTTP version of the response, the final one after any redirects and protocol upgrades (e.g. `HTTP/1.1`, `HTTP/2.0`, `HTTP/3.0`).
`peer` describes the remote peer: `address` (IP and port, when available) and `certificate` (the DER-encoded leaf certificate when the connection was TLS, as a Buffer).
Each access builds a fresh object and a fresh Buffer copy.
`trailers` is a promise of the trailing headers (see [TRL](trailers.md)).
`timing` is a promise of the request's timing breakdown as a `PerformanceResourceTiming` (see [Request timing](#request-timing)).

## Request timing

`timing` is a promise of how the request spent its time, as a genuine `PerformanceResourceTiming`.
It is the platform's own class rather than an object shaped like one, so `instanceof PerformanceResourceTiming` holds and anything that consumes performance entries takes it unmodified.

The entry is a snapshot of a finished request, so the promise settles once the response's body is finished, whether that is by being read, by being discarded, or by the collector draining an abandoned body (see [BODY](reading-the-body.md)).
A transfer that ends in an error settles it too, carrying the phases reached before the error, so the promise never hangs.
A response that cannot carry a body has finished as soon as it arrives.

Taking a response's breakdown also contributes the entry to the process's resource timeline, where a `PerformanceObserver` watching `resource` entries receives it and the resource timing buffer bounds how many are retained.
A request contributes at most one entry: the breakdown is built once and shared by the response and its clones, so asking repeatedly, or from a clone, yields that same entry.

The interface Faith exposes is the current one, which runs ahead of the platform's class: the attributes that class does not carry, along with two Faith-specific fields, are own properties of the entry, and `toJSON()` covers them so a serialised entry is complete.

Timestamps are fractional milliseconds on the same clock as `performance.now()`, so they are comparable with other performance entries, and consumers subtract one from another to obtain a duration.
A phase that did not occur, or whose boundary Faith does not observe, reads 0, as it does in a browser; the fields typed as strings, sizes, and lists read empty on the same basis.
Consumers therefore check a field for a non-zero value before differencing it.
Where a timestamp is non-zero, `fetchStart` is the earliest and the rest are no earlier than it.

`name` is the final URL after redirects, `entryType` is `resource`, and `initiatorType` is `fetch`.
`startTime` is the start of the fetch, and `duration` is `responseEnd` less `startTime`.
`responseStatus` is the response's status code, `contentType` the minimised MIME essence of `Content-Type`, and `contentEncoding` the `Content-Encoding` value (see [ENC](../fetch/content-encoding.md)).
`deliveryType` is `cache` for a response served by the HTTP cache and empty for one fetched from the network (see [CACHE](../cache/http-cache.md)).
`nextHopProtocol` is the protocol the request travelled over as an ALPN Protocol ID (RFC 7301): `h3`, `h2`, `h2c`, `http/1.1`, and so on, whether or not the connection actually negotiated over ALPN.

`serverTiming` is the metrics the origin reported in its `Server-Timing` response header, in the order the header lists them.
Each metric is one entry carrying the three attributes of a `PerformanceServerTiming`: `name` is the metric name, `duration` the fractional milliseconds its `dur` parameter reports, and `description` its `desc` parameter.
An entry is a plain object carrying those attributes rather than an instance of a platform class, and serialises as those three attributes alone.
Metrics spread across repeated `Server-Timing` header lines all contribute, and a metric name reported more than once yields one entry per occurrence.

The header is parsed to its grammar, and characters belonging to no metric name or parameter are ignored rather than ending the metric they sit in.
A metric is named by a token, so text that is no token names no metric: that metric is dropped and the rest of the list stands.
A parameter's name is a token too, matched without regard to case, and one Faith does not read is skipped rather than ending the metric.
A parameter's value is a token or a quoted string, and one that is neither, a quoted string left unterminated among them, reads empty.
A description is commonly a quoted string, and a comma or semicolon inside one belongs to the description rather than ending the metric or the list.
A metric with no `dur` reads a `duration` of 0 and one with no `desc` an empty `description`, so a metric reporting a name alone still appears.
A `dur` that is not a number reads 0, one that trails junk after a number reads that number, and a parameter given twice counts as the first of the two whether or not that first one carried a value the grammar allows.
The list is empty when the header is absent or names no metric, and a header value that is not valid UTF-8 is dropped before it is read (see [Standard properties](#standard-properties)).

The parse is held to the web platform tests for the header, so an origin's metrics read through Faith as they do in a browser.
The Server-Timing standard also carries a parsing algorithm looser than its own grammar, and the grammar and the tests are the pair Faith answers to, that being where a browser's reading of the header sits.

`fetchStart` is the moment the request began and the origin the other phases are measured against.
`finalResponseHeadersStart` is when the final response's headers arrived, and `responseEnd` when the body finished.
`responseStart` follows the standard's derivation: `firstInterimResponseStart` when that is non-zero, otherwise `finalResponseHeadersStart`.

The two additions are `reused`, whether the request travelled on a pooled connection rather than a freshly established one (see [POOL](../agent/connection-pool.md)), and `requestSent`, the moment the request head and body finished being written to the connection.
Reuse is read from the connections the agent tracks, so it is reported for the same connections `connections()` lists (see [OBS](../agent/observability.md)).
`requestSent` sits alongside the standard's `requestStart` rather than replacing it: `requestStart` marks the start of writing the request, `requestSent` the end, and the two differ by the upload time on a request carrying a body.

The service worker fields (`workerStart`, `workerRouterEvaluationStart`, `workerCacheLookupStart`, `workerMatchedRouterSource`, `workerFinalRouterSource`) and `renderBlockingStatus` describe a browsing context: they have no server-side meaning, so the timestamps read 0, the sources read empty, and `renderBlockingStatus` reads `non-blocking`.

`redirectStart`, `redirectEnd`, `domainLookupStart`, `domainLookupEnd`, `connectStart`, `connectEnd`, `secureConnectionStart`, `requestStart`, `requestSent`, and `firstInterimResponseStart` read 0, and `transferSize`, `encodedBodySize`, and `decodedBodySize` read 0.
So the span from `fetchStart` to `finalResponseHeadersStart` covers connection acquisition (pool wait, or DNS, connect, and TLS handshake on a fresh connection) together with the server's own turnaround, rather than being attributable to a phase.

`finalResponseHeadersStart` less `fetchStart` is the time to response headers.
The arrival of a response's headers is observed once, and both that field and the per-protocol path-time average for HTTP/3 slow-path demotion (see [PROBE](../http3/probing.md)) are derived from the one observation, so a request's surfaced timing and the average it feeds can never disagree about when the response arrived.
The two measure from different origins: the surfaced timing runs from the start of the request, so it covers every attempt made on the request's behalf, while the average measures the attempt that produced the response, so a path's speed is judged on its own showing.

## Threading

Response work (body reads, file writes, trailer waits) runs on Faith's own async runtime, not the libuv worker pool: concurrency is not bounded by `UV_THREADPOOL_SIZE`, and a saturated worker pool does not stall in-flight responses.
