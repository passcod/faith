# K1 · Expose a per-request timing breakdown

## Build steps

- [x] Carry the measurements: a timing slot on the response, settled when the body ends, mirroring how the trailers slot works
- [x] Stamp the response's arrival once, inside the Alt-Svc layer, and let both the path-time average and the surfaced timing derive from it
- [x] Fall back to timing the send itself when nothing stamps: a cache hit never reaches that layer, and neither does anything when HTTP/3 support is not built
- [x] Settle the timing from every route out of a body: the stream ending, `discard()`, the collector's drain, and a response that cannot carry one
- [x] Report reuse from the connection tracker, which knows whether the connection was already there
- [x] Capture the response's `Content-Encoding` before a decoded body's header is stripped
- [x] Mint the `PerformanceResourceTiming` in the wrapper, graft the attributes the platform's class lacks, and shadow `toJSON`
- [x] Memoise the entry per request and share it with clones
- [x] Types for the entry and the property
- [x] Tests, including on the oldest supported Node

## Minting a real PerformanceResourceTiming

Verified against Node v26.3.1.

`PerformanceResourceTiming` is a global but is not constructible: `new PerformanceResourceTiming()` throws `Illegal constructor`, and `Object.create(PerformanceResourceTiming.prototype)` yields an object whose getters throw `Value of "this" must be of type PerformanceResourceTiming`, so the prototype chain cannot be faked.
The only route to a genuine instance is `performance.markResourceTiming(timingInfo, requestedUrl, initiatorType, global, cacheMode, bodyInfo, responseStatus, deliveryType)`, which returns the entry and appends it to the resource timeline in the same call.
That is also what undici does for Node's own `fetch`, confirmed by observing a `resource` entry from a real `fetch` against a local server, so emitting entries is the platform-native behaviour rather than a side effect to avoid.

Because the call both mints and publishes, it must happen exactly once per request, at the point the body finishes. That is what makes `timing` a promise rather than a getter: an entry's getters read internal slots fixed at mint time, so a live-updating instance is not possible, and re-minting per access would push duplicate timeline entries.

This work lives in the JS wrapper rather than the Rust side, since `markResourceTiming` is a JS global; Rust supplies the raw measurements.

## Bridging the gap to the current interface

Node's class implements an older subset of the interface. Its prototype carries `name`, `startTime`, `duration`, `initiatorType`, `workerStart`, `redirectStart`, `redirectEnd`, `fetchStart`, `domainLookupStart`, `domainLookupEnd`, `connectStart`, `connectEnd`, `secureConnectionStart`, `nextHopProtocol`, `requestStart`, `responseStart`, `responseEnd`, `encodedBodySize`, `decodedBodySize`, `transferSize`, `deliveryType`, `responseStatus`, and `toJSON`.

Missing, and to be added as own properties on the minted entry: `finalResponseHeadersStart`, `firstInterimResponseStart`, `contentType`, `contentEncoding`, `renderBlockingStatus`, `serverTiming`, `workerRouterEvaluationStart`, `workerCacheLookupStart`, `workerMatchedRouterSource`, `workerFinalRouterSource`, plus Fáith's `reused` and `requestSent`.

Own data properties shadow prototype accessors cleanly and `instanceof` survives, both verified.
The built-in `toJSON()` ignores own properties, so it needs shadowing with an own `toJSON` that spreads the built-in result and adds the extras; otherwise `JSON.stringify` silently drops every added field.

## Sharing the instrumentation point

`run_stamped` in `src/alt_svc.rs` wraps every `next.run` and takes one instant when the response arrives.
That instant feeds `record_path_time` directly and reaches `fetch.rs` through a `HeadersStamp` in the request's extensions, so both readings come from the one observation.

The two measure from different origins on purpose. The path-time average runs from the start of the attempt that produced the response, so an HTTP/3 attempt that fails and falls back to TCP does not charge the TCP path for the time the QUIC attempt wasted. The surfaced timing runs from the start of the request, so it reports what the caller actually waited. Only a successful run stamps, which is what keeps the recorded moment attached to the response the caller receives.

A cache hit is served above this layer and never reaches it, and the layer is not built at all without the `http3` feature; both fall back to stamping where the send resolves.

## Where the body ends

The end-of-body bookkeeping was chained onto the raw byte stream, underneath the decoder. A decoder reaches the end of its own framing without necessarily polling the bytes underneath to completion, so for a decoded body that bookkeeping never ran: the trailers promise hung, `bodiesFinished` never incremented, and the timing would have hung too. It now hangs off the stream that is actually delivered, above any decoder. The trailer frames are still pulled off the raw stream, where they arrive.

## Divergences from undici worth keeping

undici sets `domainLookupStart`, `connectStart`, and `secureConnectionStart` equal to `fetchStart` when it has no real measurement, and omits `nextHopProtocol` entirely on cleartext connections.
Fáith reads 0 for phases it does not observe, which is what the standard prescribes for a phase that did not occur, and reports the ALPN Protocol ID whether or not ALPN was negotiated, which is what browsers do.
