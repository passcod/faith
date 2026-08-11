# Honour a caller's Accept-Encoding

Fáith takes ownership of content coding so the decode decision can rest on the request's `Accept-Encoding`, and so the HTTP cache stores bodies as the origin sent them.

## Why the stack forces this

The decode currently sits at the innermost layer of the stack, and the layering is what makes the card's bug unreachable by configuration.

Outermost to innermost: Fáith, the HTTP cache middleware, the Alt-Svc middleware, the reqwest client, `tower_http::Decompression`, hyper.

- reqwest 0.13 decompresses by wrapping the hyper service in `tower_http::decompression::Decompression` (`reqwest/src/async_impl/client.rs:1036`), below `reqwest-middleware` entirely.
- The decode decision reads the client's static `accept` config (`tower-http/src/decompression/service.rs:124`), never the request's header. `tower-http` inserts `Accept-Encoding` only when the header is vacant (`service.rs:116`), so a caller's value reaches the wire while having no influence on decoding. This is the defect, and no combination of settings changes it.
- reqwest removes `Content-Encoding` and `Content-Length` on decoding (documented at `client.rs:1229-1230`), which is why a `HEAD` response loses headers it should keep.
- The four decoders default to on whenever their cargo features are enabled (`client.rs:127-128`), and Fáith never disables them.
- Because decode is innermost and the cache is outside it, the cache stores already-decoded bodies whose coding headers are gone. That is why `Vary: Accept-Encoding` cannot currently mean anything: the stored entry no longer records the coding it arrived in.

Taking over the decode is therefore forced by the card. Where the decode then sits is a free choice, and outside the cache is the better one: it resolves the stored-variant question outright, keeps stored bodies compressed, and makes the header-stripping conditional on there being a body to decode.

## Cost accepted

Decoding moves from once per network response to once per delivered response, so a cache hit now pays the decode. The disk store over large bodies is where that shows. Taken knowingly in exchange for correct per-request decoding and smaller stored entries.

## Build steps

- [x] Disable reqwest's ownership of compression: drop the `gzip`, `brotli`, `deflate`, and `zstd` features in `Cargo.toml`, and confirm no `Accept-Encoding` is added beneath Fáith once they are gone (`AcceptEncoding::to_header_value` returns `None` with all four off, `tower-http/src/compression_utils.rs:44`)
- [x] Send the default `Accept-Encoding` from Fáith, matching the value the stack sent before so the wire does not change
- [x] Parse `Accept-Encoding` into a decision: coding named outright wins over `*`, a zero quality value refuses, and `*` covers what is not named. RFC 9110 §12.5.3 has `*` match "any available content coding not explicitly listed in the field"
- [x] Decode in Fáith's own layer, outside the cache middleware, driven by the request's parsed `Accept-Encoding` and the response's `Content-Encoding`
- [x] Take a direct dependency on `async-compression` for the four codings. It is in the tree today only because `tower-http` pulls it in, and reqwest's compression features are exactly `tower-http/decompression-*` (`reqwest/Cargo.toml:104,120,125,186`), so dropping them takes `async-compression`, `flate2`, `brotli`, and `zstd` out with them
- [x] Decode on the streaming path as well as the whole-body path, since every read path delivers decoded bytes
- [x] Strip `Content-Encoding` and `Content-Length` only when a body is actually decoded, so `HEAD` and bodyless responses keep them
- [x] Deliver a `Content-Encoding` naming more than one coding as received
- [x] Check the disk store across the change: entries written before it hold decoded bodies with no `Content-Encoding`, so they read back as identity. Verified against the real artefact rather than reasoned about: `test/integration/legacy-cache-encoding.test.js` installs 0.4.0 from npm, has it populate a disk store, and reads that store from the working tree. The entry is served off disk without a network trip and delivered as-is. No migration needed.

## Verified against the previous release

Running 0.4.0 alongside the working tree also settled two things that cannot be checked from one build:

- The default `Accept-Encoding` is byte-identical across the two (`zstd,gzip,deflate,br`), so the wire genuinely does not change.
- 0.4.0 decodes and strips the coding headers even when the request asked for `identity`, which pins the defect this card fixed.

## Left to its own card

Request body compression came up while planning this and is captured in the card breakdown. It rides on the compression ownership this card establishes, but carries its own option, error semantics, and 415 handling.
