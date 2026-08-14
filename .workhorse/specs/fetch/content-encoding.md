---
id: ENC
---

# Content encoding

Faith negotiates compressed transfer and decodes a response encoded in a coding the request accepted, before the caller sees any bytes.
A caller who sets `Accept-Encoding` themselves chooses what that is, and a body encoded outside that choice arrives as it came off the wire.
In the other direction the `compress` option compresses a request body in one of the same codings, opt-in per request.

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
That covers a response in a coding Faith cannot decode, and a response in a coding the request's `Accept-Encoding` did not accept.
It also covers a `Content-Encoding` naming more than one coding: Faith decodes a single coding, and a representation encoded repeatedly is the caller's to unwind.
The codings a response names are counted across every `Content-Encoding` it carries, whether they arrive comma-joined on one line or split across several, those being the same list.

## Compressing a request body

The `compress` option names the coding to compress the request body in, written as the coding's wire token: `gzip`, `deflate`, `br`, or `zstd`.
Any other value throws an `InvalidCompression` error, a `TypeError` alongside the other kinds of API misuse (see [ERR](../errors/errors.md)), and does so whether or not the request turns out to carry a body.
The four tokens are matched exactly, so `x-gzip` and `GZIP` name no coding here even though both read as gzip in a response's `Content-Encoding`: a token on the wire is taken as loosely as HTTP writes it, while an option value is an API and is refused rather than guessed at.
Each coding compresses at a level Faith picks, the same level for every request in that coding.
The option does nothing when the request has no body to compress, so a request whose body is absent or null sends no `Content-Encoding`.

Compression is opt-in per request, and Faith compresses no request body of its own accord.
Nothing in HTTP tells a caller what a server accepts in request content until a body has been sent: a server that refuses the coding answers `415` with an `Accept-Encoding` naming what it would have taken.
That response reaches the caller like any other, Faith reading nothing from it and sending nothing again in a coding it names.
The fetch standard has no equivalent of the option, so it is a Faith extension (see [FAITH](../overview.md)).

## What a compressed request sends

The coding Faith applies layers on top of whatever the caller supplied, whose own `Content-Encoding` describes the bytes they handed over.
So the request names the caller's codings followed by Faith's, in the order they were applied: a caller-set `Content-Encoding: gzip` with `compress: "zstd"` sends `Content-Encoding: gzip, zstd`, and a request with no `Content-Encoding` of its own sends the single coding Faith applied.
`Content-Length` counts the bytes Faith produced.

A `ReadableStream` body is compressed as its chunks arrive and goes out chunked with no `Content-Length`, there being no compressed length to declare before the body ends.
It remains a streaming body in every other respect, carried only over HTTP/2 and HTTP/3 unless the agent's `quirks.h1RequestStreaming` says otherwise (see [REQ](request.md)).

A 307 or 308 redirect replays the bytes Faith already compressed under the same `Content-Encoding`, compressing nothing a second time.
A 301, 302, or 303 turns the request into a `GET` and drops the body, and `Content-Encoding` goes with it as `Content-Type` does (see [REDIR](redirects.md)).

## Where decoding sits

Faith owns content coding itself rather than leaving it to the HTTP stack underneath, which is what allows the decision to rest on the `Accept-Encoding` of the request in hand.
Decoding happens outside the HTTP cache, so a stored response holds the bytes as they came off the wire with its `Content-Encoding` and `Content-Length` as received (see [CACHE](../cache/http-cache.md)).
A response served from the cache is decoded on its way to the caller under the `Accept-Encoding` of the request being served, so one stored entry answers callers who negotiated different codings, and a cache holds compressed bodies at the size the origin sent them.
