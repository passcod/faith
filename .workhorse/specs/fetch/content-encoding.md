---
id: ENC
---

# Content encoding

Fáith negotiates compressed transfer and decodes a response encoded in a coding the request accepted, before the caller sees any bytes.
A caller who sets `Accept-Encoding` themselves chooses what that is, and a body encoded outside that choice arrives as it came off the wire.

## Negotiation

A request the caller adds no `Accept-Encoding` to carries one advertising zstd, gzip, deflate, and brotli.
An `Accept-Encoding` from the caller, whether set on the request or as an agent default header, replaces that value and is sent as given, `identity` and quality values included (see [REQ](request.md)).

## Decoding

A response is decoded when its `Content-Encoding` is zstd, gzip, deflate, or brotli, and the request's `Accept-Encoding` accepted that coding: named outright or matched by `*`, without a zero quality value.
So a caller asking for gzip alone receives a gzip response decoded, while a server that compresses in the face of `Accept-Encoding: identity` hands the caller the compressed bytes.
Whether to decode is decided for each request from the `Accept-Encoding` that request carried, not from a setting the agent holds for all of them.
`Content-Encoding` and `Content-Length` are removed from the response headers on decoding, so the headers the caller reads describe the bytes the caller receives.
Decoding applies to every way of reading the body, the `body` stream included (see [BODY](../response/reading-the-body.md)).

## Bodies delivered as received

Every other response is delivered as received, with `Content-Encoding` and `Content-Length` intact, leaving the caller to decode the bytes.
That covers a response in a coding Fáith cannot decode, and a response in a coding the request's `Accept-Encoding` did not accept.
