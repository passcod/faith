# Honour a caller's Accept-Encoding

Follow-on work surfaced while specifying how a caller's `Accept-Encoding` governs decoding.

## Custom content decoders on the agent · D2

An agent option lets a caller supply their own decoders, each registered against a content coding.
Fáith advertises the callers' codings in `Accept-Encoding` alongside the ones it decodes itself, and a coding the caller has registered a decoder for is handed to that decoder rather than the built-in one.
This covers codings Fáith has no decoder for, and gives a caller who wants the bytes as sent a way to get them: registering a passthrough decoder for `gzip` yields the gzip bytes while still advertising gzip to the server.

## Request body compression · E2

A per-request option compresses the request body in one of the codings Fáith owns, setting `Content-Encoding` and sizing `Content-Length` to the compressed bytes.
Compression is opt-in per request rather than automatic, because nothing in HTTP tells a caller what a server accepts in request content until after a body has been sent: a server that refuses the coding answers `415` with an `Accept-Encoding` naming what it would have taken.
A `ReadableStream` body goes out chunked, there being no compressed length to declare up front.
The fetch standard has no equivalent, so the option is a Fáith extension.
