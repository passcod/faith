---
id: QUIC
---

# HTTP/3 transport

QUIC transport tuning lives under the agent's `http3` options alongside the upgrade machinery.
These settings shape established HTTP/3 connections rather than how origins get upgraded.

`http3.congestion` selects the congestion control algorithm for the upload direction (the server controls download): `cubic` (default, fair to all traffic) or `bbr1` (maximises bandwidth use and tolerates loss, at the cost of flooding lossy networks with retransmissions).
The choice only matters for upload-heavy traffic.
`http3.maxIdleTimeout` (seconds, default 30) sets this side's idle timeout; the effective timeout is the minimum of both peers'.
Values are clamped to between 1 second and 2 minutes for safety.
The QUIC socket binds the IPv6 wildcard by default, falling back to the IPv4 wildcard on hosts that cannot bind it (see [AGENT](../agent/overview.md)); TLS 1.3 early data is available for HTTP/3 via `tls.earlyData` (see [TLS](../agent/tls.md)).

## Flow control

QUIC flow-control windows bound how much data an origin may send before Faith acknowledges it, trading connection memory for throughput on high-latency links.
HTTP/3 connections open with a 6 MiB per-stream receive window and a 15 MiB whole-connection receive window, the same shape and reasoning as HTTP/2 (see [H2](../agent/http2.md)), so throughput does not change when an origin upgrades from one protocol to the other.
Bounding the connection window also bounds the worst-case buffering of a connection carrying many concurrent requests, which a per-stream window alone does not.

`http3.streamWindow` and `http3.connectionWindow` override the per-stream and whole-connection receive windows, each in bytes.
`http3.sendWindow` (bytes) caps how much data Faith transmits without acknowledgement, bounding upload throughput the way the receive windows bound download; the origin's own flow control applies on top of it.
