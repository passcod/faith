# Follow-ups from the per-request timing breakdown

The timing breakdown mirrors the whole `PerformanceResourceTiming` interface, with the phases Fáith does not observe reading 0 as they do in a browser.
Each card below fills one group of those fields in.
The service worker and render-blocking fields are not among them: they describe a browsing context and stay empty by design.

## Populate the request-writing timestamps

Give `requestStart` and `requestSent` real values, splitting the `fetchStart` to `finalResponseHeadersStart` span into time spent writing the request and time spent waiting on the server.
reqwest gives no signal for a request head or body having been written, so this needs the outgoing body wrapped to observe its first and last poll, and a decision on what to report for requests whose body is empty or not constructed by Fáith.

## Populate the DNS, connect, and TLS phases

Give `domainLookupStart`, `domainLookupEnd`, `connectStart`, `connectEnd`, and `secureConnectionStart` real values so connection acquisition can be attributed to a phase rather than read as one span.
These boundaries are not reachable without hooks upstream in reqwest or hyper, so this likely starts as an upstream contribution.
It pairs with `reused`, which already says whether setup happened at all.

## Populate the redirect phases

Give `redirectStart` and `redirectEnd` real values for requests that followed redirects, so the cost of the redirect chain is separable from the final request's own time.
Redirects are followed internally (see the redirects spec), so the boundaries are Fáith's own to record, but the timing breakdown is currently assembled from the final request alone.

## Populate the interim response timestamp

Give `firstInterimResponseStart` a real value when the server sends a 1xx interim response, which also makes `responseStart` derive from it rather than always falling through to `finalResponseHeadersStart`.
This is what surfaces the benefit of `103 Early Hints`, and it needs interim responses to be observable in the first place, which reqwest does not currently expose.

## Populate the transfer and body sizes

Give `transferSize`, `encodedBodySize`, and `decodedBodySize` real values, counted at the point Fáith decodes the body so the encoded and decoded figures come from the same read.
`transferSize` additionally counts response header bytes, which needs a wire-level count rather than a body-level one, and the standard's convention for a cache hit (0 when served locally) has to line up with how the HTTP cache serves responses.

## Populate server timing

Parse the `Server-Timing` response header into `serverTiming` as a list of entries carrying `name`, `duration`, and `description`, matching `PerformanceServerTiming`.
This is a header parse rather than an instrumentation change, and it is the one field in the group that does not depend on anything upstream.
