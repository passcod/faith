---
id: ENC
---

# Content encoding

Fáith negotiates compressed transfer on every request and decodes the response before the caller sees any bytes.
Callers work with decoded bodies and headers that describe those decoded bytes, without opting in.

## Negotiation

Every request carries `Accept-Encoding` advertising zstd, gzip, deflate, and brotli.
A caller-supplied `Accept-Encoding` replaces that default and is sent as given (see [REQ](request.md)).

## Decoding

A response whose `Content-Encoding` is one of the four negotiated encodings is decoded, whatever the request advertised.
`Content-Encoding` and `Content-Length` are removed from the response headers on decoding, so the headers the caller reads describe the bytes the caller receives.
Decoding applies to every way of reading the body, the `body` stream included (see [BODY](../response/reading-the-body.md)).
A response in any other encoding is delivered as received, with its `Content-Encoding` header intact.
