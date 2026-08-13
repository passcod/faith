---
id: FLOW
---

# Flow control

Flow-control windows bound how much data an origin may send before Faith acknowledges it, trading connection memory for throughput.
Larger windows keep a high-latency link full, so the receiver is not the bottleneck; smaller windows hold less buffered data per connection.
HTTP/2 and HTTP/3 both carry their own flow control, at the HTTP/2 framing layer and the QUIC transport layer respectively, so both are tuned here.
HTTP/1 has no flow control of its own and is governed by the operating system's TCP receive window.

## Common windows

`flowControl.streamWindow` and `flowControl.connectionWindow`, each in bytes, set the per-stream and whole-connection receive windows for HTTP/2 and HTTP/3 alike.
Setting these is how the windows are normally tuned: one value applies to whichever protocol a request negotiates, so throughput does not change when an origin upgrades from one protocol to the other.

They default to a 6 MiB per-stream window and a 15 MiB whole-connection window, following browser practice.
The connection window is larger than the stream window so concurrent streams on one connection share the connection's headroom while each stream's worst-case buffering stays capped.
Bounding the connection window also bounds the worst-case buffering of a connection carrying many concurrent requests, which a per-stream window alone does not.
The defaults sit at the conservative end of browser practice rather than at peak measured throughput: a pooled server-side client can hold many connections across many origins, so per-connection memory multiplies harder than it does for a browser holding few connections to few origins.

## Per-protocol windows

`http2.streamWindow`, `http2.connectionWindow`, `http3.streamWindow`, and `http3.connectionWindow` set the windows for one protocol only, in bytes.
A per-protocol window takes precedence over the common one, so setting `flowControl.streamWindow` alongside `http3.streamWindow` gives HTTP/3 the latter and leaves HTTP/2 on the former.
These exist for tuning one protocol against the other; a caller who wants both to behave the same sets the common windows instead.

`http3.sendWindow` (bytes) caps how much data Faith transmits without acknowledgement, bounding upload throughput the way the receive windows bound download; the origin's own flow control applies on top of it.

## Adaptive windowing

`http2.adaptiveWindow` (default `false`) replaces HTTP/2's static windows with windows that start small and grow towards a bandwidth-delay estimate sampled from connection pings, capped at 16 MiB.

Adaptive windowing and explicit windows are mutually exclusive: enabling it ignores both the common and the HTTP/2 windows, because the estimator owns them itself.
HTTP/3 is unaffected and keeps whichever windows apply to it, so an agent with adaptive windowing on still honours `flowControl.streamWindow` for its HTTP/3 connections.
Adaptive windowing is off by default because a fresh connection opens far below the static default and takes many round trips to ramp up, so it carries less throughput than the static window for all but the largest transfers, and turning it on gives up the ability to tune the windows explicitly.
