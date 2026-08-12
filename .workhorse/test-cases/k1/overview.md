# K1 test cases · Per-request timing breakdown

Scenarios verifying the timing breakdown a response carries.
Automated cases live in `test/timing.test.js` and run against httpbin.

## The entry itself

- [x] `timing` resolves to a genuine `PerformanceResourceTiming`, with `entryType` `resource`, `initiatorType` `fetch`, and `name` the final URL (verifies spec: RESP)
- [x] Phases sit on the `performance.now()` clock: `fetchStart` is no earlier than a reading taken before the request, `responseEnd` no later than one taken after (verifies spec: RESP)
- [x] `fetchStart` is the earliest phase, and `responseEnd` is no earlier than `finalResponseHeadersStart` (verifies spec: RESP)
- [x] `duration` is `responseEnd` less `startTime` (verifies spec: RESP)
- [x] `responseStart` falls through to `finalResponseHeadersStart` when no interim response arrived (verifies spec: RESP)
- [x] Every field the entry carries survives `JSON.stringify`, including the ones beyond the platform's class (verifies spec: RESP)

## Values

- [x] `responseStatus`, `contentType` (MIME essence, parameters dropped), and `nextHopProtocol` report the response's own details (verifies spec: RESP)
- [x] `contentEncoding` reports the coding a decoded body arrived under, even though the decoded response no longer carries the header (verifies spec: RESP)
- [x] `deliveryType` is empty for a network response (verifies spec: RESP)
- [ ] `deliveryType` is `cache` for a response served by the HTTP cache (verifies spec: RESP)
- [x] `reused` is false for the first request on a connection and true for one that followed on the pooled connection (verifies spec: RESP)
- [x] The phases Fáith does not observe read 0, and `serverTiming` is empty (verifies spec: RESP)
- [x] The browsing context fields read empty, with `renderBlockingStatus` reading `non-blocking` (verifies spec: RESP)
- [ ] `nextHopProtocol` reads `h2` over TLS and `h2c` in cleartext (verifies spec: RESP)

## Settling

- [x] The promise settles once the body has been read (verifies spec: RESP)
- [x] The promise settles once the body has been discarded (verifies spec: RESP)
- [x] The promise settles for a response that cannot carry a body (verifies spec: RESP)
- [x] A decoded body settles the promise, rather than leaving it pending because the decoder finished above the raw bytes (verifies spec: RESP)
- [ ] The promise settles when an abandoned body is drained by the collector (verifies spec: RESP)
- [ ] A transfer that ends in an error settles the promise, carrying the phases reached before it (verifies spec: RESP)

## The resource timeline

- [x] A `PerformanceObserver` watching `resource` entries receives the entry, and it is the same object the promise resolved to (verifies spec: RESP)
- [x] A request contributes exactly one timeline entry, shared between the response, its clones, and repeated access (verifies spec: RESP)

## Alongside

- [x] Trailers settle for a decoded body, which shares the end-of-body bookkeeping the timing uses
- [ ] The path-time average and the surfaced timing agree on when a response arrived (verifies spec: PROBE)
- [x] The breakdown behaves the same on the oldest supported Node, whose `PerformanceResourceTiming` implements fewer attributes (verified by hand on Node 20)
