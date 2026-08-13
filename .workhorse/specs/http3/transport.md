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

QUIC flow-control windows are shared with HTTP/2 rather than specified here: the `http3.streamWindow`, `http3.connectionWindow`, and `http3.sendWindow` options sit alongside the common windows that cover both protocols (see [FLOW](../agent/flow-control.md)).
