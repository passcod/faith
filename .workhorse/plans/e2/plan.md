# Request body compression

Implements the `compress` option from ENC, "Compressing a request body" and "What a compressed request sends".

## Technical notes

**Where compression sits.** Faith already owns the four codings for decoding, in `src/encoding.rs`, over `async-compression`'s tokio `bufread` adapters. The encoders come from the same crate under the same features (`gzip`, `zlib`, `brotli`, `zstd`), so compression joins that module rather than starting a new one.

**Header layering needs the caller's value withheld.** reqwest's `RequestBuilder::header` *appends* rather than inserts, so letting the caller's `Content-Encoding` through the main header loop and then adding Faith's would emit two lines. ENC's own decoding rules treat several `Content-Encoding` lines as one list, so that reads as the caller's codings twice over. The caller's value is therefore withheld in the loop and re-emitted once, joined with Faith's token.

**The agent's default headers are a second source.** reqwest fills a default header in only where the request carries none of that name, so setting a combined `Content-Encoding` on the request would silently displace an agent default. The agent captures its default `Content-Encoding` up front, as it already does for `Accept-Encoding` and `Priority`.

**Redirects need no code.** reqwest routes redirects through tower-http's follow-redirect, whose `drop_payload_headers` removes `Content-Encoding` alongside `Content-Type` and `Content-Length` when a 301, 302, or 303 drops the body. A 307 or 308 replays the body untouched, which is the already-compressed bytes under the header as sent.

**`Content-Length` needs no code either.** A buffered body is handed to reqwest as bytes, so the length it derives is the compressed length. A streaming body goes through `Body::wrap_stream` and is chunked with no length, as it is today.

**The option value is matched exactly.** `Coding::from_token` accepts `x-gzip` and matches case-insensitively because that is what a wire token demands; the option is an API surface, so it matches the four documented tokens exactly, following how `priority` treats `HIGH` as unrecognised.

## Build steps

- [x] Add the `InvalidCompression` error kind to `src/error.rs`, in the `TypeError` group, with its doc-comment entry
- [x] Add `Coding::token()` and an exact-match option parser to `src/encoding.rs`
- [x] Add `compress_buffer` and `compress_stream` to `src/encoding.rs`
- [x] Thread a `compress` option through `src/options.rs`
- [x] Capture the agent's default `Content-Encoding` in `src/agent.rs`
- [x] Apply compression and the layered `Content-Encoding` in `src/fetch.rs`, both body paths
- [x] Update the README's error-code list and its `FetchOptions` reference, and declare `compress` and `InvalidCompression` in `wrapper.d.ts` (the hand-written public surface, which `test/error-codes.test.js` checks against the native codes)
- [x] Rust unit tests for token parsing, round-trip compression, and header layering
- [x] JS tests over a `/sink` route added to the encoding origin, covering the buffered, streaming, layering, and redirect paths
- [x] Run `cargo fmt`, `cargo test`, and the full JS suite
