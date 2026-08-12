# K1 · Expose a per-request timing breakdown

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

The path-time EWMA already measures time-to-response-headers at `src/alt_svc.rs:974`, wrapping `next.run(req)` and feeding `record_path_time`.
The timing breakdown reads `fetchStart` to `finalResponseHeadersStart` from that same measurement so the two never diverge.

## Divergences from undici worth keeping

undici sets `domainLookupStart`, `connectStart`, and `secureConnectionStart` equal to `fetchStart` when it has no real measurement, and omits `nextHopProtocol` entirely on cleartext connections.
Fáith reads 0 for phases it does not observe, which is what the standard prescribes for a phase that did not occur, and reports the ALPN Protocol ID whether or not ALPN was negotiated, which is what browsers do.
