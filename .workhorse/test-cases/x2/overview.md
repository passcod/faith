# X2 test cases

Scenarios verifying that a streaming request body is reserved for HTTP/2 and HTTP/3, and that the agent quirk opts back out of that.

The httpbin origin the suite runs against is plain HTTP/1.1, so it serves as the HTTP/1.x side throughout. Cases live in `test/h1-request-streaming.test.js` unless noted.

## The rule

- [x] A `ReadableStream` body posted to an HTTP/1.1 origin fails, carrying the `Network` code and the `NetworkError` shape (verifies spec: REQ, ERR)
- [x] The failure message names `quirks.h1RequestStreaming` as the way to allow it (verifies spec: REQ)
- [x] An agent constructed with no options refuses, so the compliant behaviour is the default (verifies spec: QUIRK)
- [x] A refused request reaches the origin not at all: no request line, no body bytes (verifies spec: REQ)
- [x] An HTTPS origin whose ALPN settles on HTTP/1.1 refuses too, and sees no request (verifies spec: REQ)
- [x] A `ReadableStream` body succeeds over an HTTP/2 origin with no quirk set, every byte arriving (verifies spec: REQ)

## The quirk

- [x] With `quirks.h1RequestStreaming` on, a streamed body posts to an HTTP/1.1 origin and returns 200 (verifies spec: QUIRK)
- [x] With the quirk on, every byte of a multi-chunk streamed body arrives at the origin (verifies spec: QUIRK)
- [x] The quirk applies only to the agent it is set on: a second agent without it still refuses (verifies spec: QUIRK)
- [x] The quirk leaves an already-eligible HTTP/2 origin behaving as it did (verifies spec: QUIRK)

## Releasing the stream on a refusal

- [x] After a refusal, a process whose stream keeps producing chunks still exits on its own, rather than the chunk pump stranding on a channel nobody reads (verifies spec: REQ)

## Bodies the rule does not touch

- [x] A string body still posts over HTTP/1.1, body intact (verifies spec: REQ)
- [x] A body supplied through a `Request` object posts over HTTP/1.1, being buffered during conversion (verifies spec: REQ)
- [x] The existing streaming coverage still passes with the quirk on: chunking, large payloads, async chunks, binary data, empty streams, header preservation, abort mid-stream (`test/stream-body.test.js`, `test/duplex.test.js`)
- [x] The `priority` option still applies to a streaming request (`test/priority.test.js`, verifies spec: REQ)
- [x] Full duplex over HTTP/1.1 still holds for an agent with the quirk on, across all three sequencing cases (`test/duplex-sequencing.test.js`, verifies spec: REQ)

## Notes

The TLS cases stand up a local origin pinned to one ALPN protocol, using the certificate fixture the HTTP/3 tests already share. They matter because the plaintext refusal is decided from the scheme alone, while the TLS ones go through the transport check that runs once ALPN has settled, which is a different path through the code.
