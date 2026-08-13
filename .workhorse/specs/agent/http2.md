---
id: H2
---

# HTTP/2 flow control

HTTP/2 flow-control windows bound how much data an origin may send before Faith acknowledges it, trading connection memory for throughput.
Larger windows keep a high-latency link full, so the receiver is not the bottleneck; smaller windows hold less buffered data per connection.
These settings live under the agent's `http2` options and shape every HTTP/2 connection the agent opens.

## Static windows

By default Faith opens each HTTP/2 connection with large static flow-control windows: a 6 MiB per-stream receive window and a 15 MiB whole-connection receive window, following browser practice.
The connection window is larger than the stream window so concurrent streams on one connection share the connection's headroom while each stream's worst-case buffering stays capped.
The defaults sit at the conservative end of browser practice rather than at peak measured throughput: a pooled server-side client can hold many connections across many origins, so per-connection memory multiplies harder than it does for a browser holding few connections to few origins.

`http2.streamWindow` and `http2.connectionWindow` override the per-stream and whole-connection windows, each in bytes.

## Adaptive windowing

`http2.adaptiveWindow` (default `false`) replaces the static windows with windows that start small and grow towards a bandwidth-delay estimate sampled from connection pings, capped at 16 MiB.

Adaptive windowing and explicit windows are mutually exclusive: enabling `http2.adaptiveWindow` ignores `http2.streamWindow` and `http2.connectionWindow`, because the estimator owns both windows itself.
Adaptive windowing is off by default because a fresh connection opens far below the static default and takes many round trips to ramp up, so it carries less throughput than the static window for all but the largest transfers, and turning it on gives up the ability to tune the windows explicitly.
