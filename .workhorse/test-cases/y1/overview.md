# Honour a caller's Accept-Encoding

Concrete scenarios for the decode decision resting on the request's `Accept-Encoding`.

Coverage lives in three places: the decision logic in `src/encoding.rs` unit tests, behaviour against go-httpbin in `test/compression.test.js`, and behaviour against our own origin in `test/content-encoding.test.js`. The custom origin exists because go-httpbin compresses on its own terms and has no route for a layered coding, a cacheable coded response, or a `HEAD` describing a compressed representation.

## Negotiation

- [x] A request with no `Accept-Encoding` advertises `zstd,gzip,deflate,br` on the wire (verifies spec: ENC)
- [x] A caller-supplied `Accept-Encoding` is sent as given, `identity` and quality values included (verifies spec: ENC, REQ)
- [x] An agent default-header `Accept-Encoding` is sent when the request adds none, and governs the decode (verifies spec: ENC)
- [x] A request header replaces the agent default, and governs the decode with it (verifies spec: ENC, REQ)
- [x] Agent default headers that do not name `Accept-Encoding` leave Fáith's default in place (verifies spec: ENC)

## Decoding

- [x] Each of gzip, deflate, br, and zstd is decoded when the request accepted it (verifies spec: ENC)
- [x] Decoding strips `Content-Encoding` and `Content-Length` (verifies spec: ENC)
- [x] The `body` stream delivers decoded bytes, not just the whole-body methods (verifies spec: ENC, BODY)
- [x] A coding named outright with a zero quality value is refused even when `*` accepts (`gzip;q=0, *`) (verifies spec: ENC)
- [x] `*` covers a coding not named outright (verifies spec: ENC)
- [x] A quality value parses to thousandths, so `q=0.001` still accepts (verifies spec: ENC)

## Bodies delivered as received

- [x] `Accept-Encoding: identity` against a compressed response yields the bytes as sent, `Content-Encoding` and `Content-Length` intact (verifies spec: ENC)
- [x] A coding the request did not accept is delivered as received (verifies spec: ENC)
- [x] A coding Fáith cannot decode is delivered as received, bytes untouched (verifies spec: ENC)
- [x] The `body` stream of an undecoded response carries the encoded bytes (verifies spec: ENC, BODY)
- [x] A `HEAD` response keeps `Content-Encoding` and `Content-Length`, sized to the encoded representation (verifies spec: ENC)
- [x] A bodyless response whose coding the request refused keeps both headers (verifies spec: ENC)
- [x] The digest of an integrity check is taken over the encoded bytes on a response Fáith does not decode, and over the decoded bytes on one it does (verifies spec: INT, ENC)

## Layered codings

- [x] `Content-Encoding: gzip, br` on one line is delivered as received, for the caller to unwind (verifies spec: ENC)
- [x] The same two codings split across separate `Content-Encoding` lines count as one list, so neither is decoded (verifies spec: ENC)
- [x] A single coding split across lines alongside an empty one still decodes (verifies spec: ENC)
- [x] A `Content-Encoding` line that is not valid ASCII is delivered as received (verifies spec: ENC)
- [x] `identity, gzip` counts as more than one coding and is delivered as received (verifies spec: ENC)

## Cache

- [x] A stored response holds the bytes as they came off the wire and is decoded on the way out (verifies spec: ENC, CACHE)
- [x] With no `Vary`, one stored entry answers both a caller who negotiated gzip and one who asked for identity, decoded for the first and as sent for the second (verifies spec: ENC, CACHE)
- [x] A disk-store entry round-trips its coding across a hit (verifies spec: ENC, CACHE)
- [ ] A disk-store entry written before this change reads back as identity. Not automated: it needs a store populated by the previous build, so it is a manual check against an existing cache directory. Pre-change entries hold decoded bodies with no `Content-Encoding`, so the decision returns "no coding" and they are delivered as-is.
