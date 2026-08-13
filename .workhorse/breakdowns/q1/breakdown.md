# Full duplex mode breakdown

Work this card surfaced that belongs on its own card rather than in the duplex design.

## Streaming request bodies over HTTP/1.1

The fetch standard rules out a streaming request body on an HTTP/1.x connection: "If connection is an HTTP/1.x connection, request's body is non-null, and request's body's source is null, then return a network error", where a null source means the body came from a `ReadableStream`. This is why browsers require HTTP/2 for streaming uploads. Faith streams request bodies over HTTP/1.1 without complaint, and full duplex works there, measured over plain HTTP/1.1 against a local origin. The divergence looks deliberate rather than accidental, but it is undocumented and unspecified, and nothing in the suite pins it. Decide whether Faith keeps the capability, and if so specify it and cover it with tests; the standard's reason for the restriction (an HTTP/1.1 request body cannot be replayed, so a request that must be resent after a dead connection cannot be) bears on how it interacts with the connection-pool replay bound in [POOL](../../specs/agent/connection-pool.md).
