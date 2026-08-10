---
id: ENC
---

# Content encoding

Fáith negotiates compressed transfer on a request the caller left alone, and decodes that response before the caller sees any bytes.
A caller who sets `Accept-Encoding` themselves takes over the negotiation, and receives the body as it came off the wire.

## Negotiation

A request the caller adds no `Accept-Encoding` to carries one advertising zstd, gzip, deflate, and brotli.
An `Accept-Encoding` from the caller, whether set on the request or as an agent default header, replaces that value and is sent as given (see [REQ](request.md)).
Any value is honoured, `identity` included, so a caller can ask for the resource uncompressed.

## Decoding

Fáith decodes only the responses whose encoding it negotiated itself: a response to a request carrying Fáith's own `Accept-Encoding`, whose `Content-Encoding` is one of the four encodings that header advertises.
`Content-Encoding` and `Content-Length` are removed from the response headers on decoding, so the headers the caller reads describe the bytes the caller receives.
Decoding applies to every way of reading the body, the `body` stream included (see [BODY](../response/reading-the-body.md)).

## Bodies delivered as received

Every other response is delivered as received, with `Content-Encoding` and `Content-Length` intact, leaving the caller to decode the bytes if they want to.
That covers a response in an encoding outside the four Fáith advertises, and every response to a request whose `Accept-Encoding` the caller supplied.
