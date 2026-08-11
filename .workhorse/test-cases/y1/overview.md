# Honour a caller's Accept-Encoding

Concrete scenarios for the decode decision resting on the request's `Accept-Encoding`. Automated cases live in `test/compression.test.js` and the Rust unit tests in `src/encoding.rs`. Server behaviour comes from go-httpbin, whose `/gzip` and `/deflate` compress regardless of what the request advertised.

## Negotiation

- [x] A request with no `Accept-Encoding` advertises `zstd,gzip,deflate,br` on the wire (verifies spec: ENC)
- [x] A caller-supplied `Accept-Encoding` is sent as given, `identity` and quality values included (verifies spec: ENC, REQ)
- [ ] An agent default-header `Accept-Encoding` is sent when the request adds none (verifies spec: ENC)

## Decoding

- [x] A response in a coding the request accepted is decoded, and `Content-Encoding`/`Content-Length` are stripped (verifies spec: ENC)
- [x] The `body` stream delivers decoded bytes, not just the whole-body methods (verifies spec: ENC, BODY)
- [x] A coding named outright with a zero quality value is refused even when `*` accepts (`gzip;q=0, *`) (verifies spec: ENC)
- [x] `*` covers a coding not named outright (verifies spec: ENC)
- [x] A quality value parses to thousandths, so `q=0.001` still accepts (verifies spec: ENC)

## Bodies delivered as received

- [x] `Accept-Encoding: identity` against a compressed response yields the bytes as sent, `Content-Encoding` and `Content-Length` intact (verifies spec: ENC)
- [x] A coding the request did not accept is delivered as received (verifies spec: ENC)
- [x] A `Content-Encoding` naming more than one coding is delivered as received (verifies spec: ENC)
- [x] A coding Fáith cannot decode is delivered as received (verifies spec: ENC)
- [ ] A `HEAD` response keeps `Content-Encoding` and `Content-Length`, nothing having been decoded (verifies spec: ENC)
- [ ] The digest of an integrity check is taken over the encoded bytes on a response Fáith does not decode (verifies spec: INT)

## Cache

- [ ] A stored response holds the bytes as they came off the wire, its `Content-Encoding` intact, and is decoded on the way out (verifies spec: ENC, CACHE)
- [ ] One stored entry answers callers who negotiated different codings (verifies spec: ENC, CACHE)
- [ ] A disk-store entry written before this change reads back as identity (verifies spec: ENC)
