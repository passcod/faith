# Request body compression

Scenarios verifying the `compress` option. The automated cases run against the encoding origin in `test/fixtures/encoding-origin.js`, whose `/sink` route reports the bytes and headers a request actually arrived with.

## The option

- [x] Each of `gzip`, `deflate`, `br`, and `zstd` compresses the body and names itself in `Content-Encoding` (verifies spec: ENC)
- [x] The compressed bytes decode back to exactly what the caller supplied (verifies spec: ENC)
- [x] `Content-Length` counts the compressed bytes, not the bytes handed in (verifies spec: ENC)
- [x] A request without the option sends no `Content-Encoding` and its body goes out as given (verifies spec: ENC)
- [x] A value naming no coding throws `InvalidCompression` as a `TypeError` (verifies spec: ENC, ERR)
- [x] The tokens match exactly, so `x-gzip`, `GZIP`, `brotli`, `identity`, and the empty string all throw (verifies spec: ENC)
- [x] A bad value throws even on a request with no body to compress (verifies spec: ENC)
- [x] The option does nothing on a request whose body is absent or null (verifies spec: ENC)

## Layering

- [x] A caller-set `Content-Encoding: gzip` with `compress: "zstd"` sends `Content-Encoding: gzip, zstd` (verifies spec: ENC)
- [x] Faith compresses the bytes it was handed, not what those bytes decode to (verifies spec: ENC)
- [x] The joined value arrives as one header line, not the caller's coding repeated (verifies spec: ENC)
- [x] An agent's default `Content-Encoding` is layered on rather than displaced (verifies spec: ENC)
- [x] A per-request `Content-Encoding` beats the agent's default before Faith's coding is added (verifies spec: ENC, REQ)

## Streaming

- [x] A `ReadableStream` body goes out chunked with no `Content-Length`, under the coding it names (verifies spec: ENC)
- [x] The chunks decode to what was written across them (verifies spec: ENC)
- [x] A compressed streaming body over HTTP/1.1 is refused without `quirks.h1RequestStreaming`, as an uncompressed one is (verifies spec: REQ)

## Across a redirect

- [x] A 307 replays the already-compressed bytes under the same `Content-Encoding`, unwinding in one pass (verifies spec: ENC, REDIR)
- [x] A 303 turns the request into a `GET` and drops `Content-Encoding` with the body (verifies spec: ENC, REDIR)

## Operational

- [x] A server refusing the coding answers `415` and that response reaches the caller unchanged, with no second request sent (verifies spec: ENC)
- [ ] Compression works over HTTP/2 and HTTP/3, not only the cleartext HTTP/1.1 the encoding origin serves
- [ ] A large body (tens of MB) compresses without exhausting memory or stalling the request
