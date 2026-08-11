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
A coding the header names outright settles the question whatever `*` says, so `gzip;q=0, *` refuses gzip while accepting the other three.
So a caller asking for gzip alone receives a gzip response decoded, while a server that compresses in the face of `Accept-Encoding: identity` hands the caller the compressed bytes.
Whether to decode follows from the `Accept-Encoding` the request carried, one inherited from an agent default header included, and not from any separate decoding setting.
`Content-Encoding` and `Content-Length` are removed from the response headers on decoding, so the headers the caller reads describe the bytes the caller receives.
Removing them is a knowing divergence from the fetch standard, which decodes the body and leaves the header list as it was (see [FAITH](../overview.md)).
A response that cannot carry a body keeps both headers, nothing having been decoded, so a `HEAD` response still describes the representation a `GET` would return.
Decoding applies to every way of reading the body, the `body` stream included (see [BODY](../response/reading-the-body.md)).

## Bodies delivered as received

Every other response is delivered as received, with `Content-Encoding` and `Content-Length` intact, leaving the caller to decode the bytes.
That covers a response in a coding Fáith cannot decode, and a response in a coding the request's `Accept-Encoding` did not accept.
It also covers a `Content-Encoding` naming more than one coding: Fáith decodes a single coding, and a representation encoded repeatedly is the caller's to unwind.
The codings a response names are counted across every `Content-Encoding` it carries, whether they arrive comma-joined on one line or split across several, those being the same list.

## Where decoding sits

Fáith owns content coding itself rather than leaving it to the HTTP stack underneath, which is what allows the decision to rest on the `Accept-Encoding` of the request in hand.
Decoding happens outside the HTTP cache, so a stored response holds the bytes as they came off the wire with its `Content-Encoding` and `Content-Length` as received (see [CACHE](../cache/http-cache.md)).
A response served from the cache is decoded on its way to the caller under the `Accept-Encoding` of the request being served, so one stored entry answers callers who negotiated different codings, and a cache holds compressed bodies at the size the origin sent them.
