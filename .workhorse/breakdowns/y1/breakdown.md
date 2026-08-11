# Honour a caller's Accept-Encoding

Follow-on work surfaced while specifying how a caller's `Accept-Encoding` governs decoding.

## Custom content decoders on the agent

An agent option lets a caller supply their own decoders, each registered against a content coding.
Fáith advertises the callers' codings in `Accept-Encoding` alongside the ones it decodes itself, and a coding the caller has registered a decoder for is handed to that decoder rather than the built-in one.
This covers codings Fáith has no decoder for, and gives a caller who wants the bytes as sent a way to get them: registering a passthrough decoder for `gzip` yields the gzip bytes while still advertising gzip to the server.
