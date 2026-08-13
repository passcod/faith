# Flow-control windows

Scenarios verifying the common `flowControl` group, the per-protocol overrides, and HTTP/2
adaptive windowing (verifies spec: FLOW).

The HTTP/2 cases assert on what the origin is actually told: a Node HTTP/2 server reads the
client's per-stream window from its SETTINGS and the connection window from the session's remote
window size. The HTTP/3 cases cannot do the same, because QUIC carries the windows in transport
parameters and Caddy does not report the ones its peer sent, so they assert that a transfer
completes under the configured windows instead.

## Defaults

- [x] An agent with no options advertises a 6 MiB per-stream window over HTTP/2
- [x] An agent with no options advertises a 15 MiB whole-connection window over HTTP/2
- [x] The connection window is larger than the stream window
- [x] An HTTP/3 request completes on the default windows

## Common windows

- [x] `flowControl.streamWindow` and `flowControl.connectionWindow` reach an HTTP/2 origin
- [x] An HTTP/3 request completes under the common windows
- [ ] One `flowControl` value produces the same windows on an origin reached over HTTP/2 and the same origin reached over HTTP/3

## Per-protocol overrides

- [x] `http2.streamWindow` takes precedence over `flowControl.streamWindow`
- [x] A protocol override of one window leaves the other on the common value
- [x] `http2.streamWindow` and `http2.connectionWindow` apply with no `flowControl` group set
- [x] `http3.streamWindow`, `http3.connectionWindow`, and `http3.sendWindow` apply over HTTP/3
- [x] Overriding one protocol's window leaves the other protocol on the common value
- [ ] `http3.streamWindow` reaches the origin as a QUIC transport parameter
- [ ] `http3.sendWindow` bounds an upload, and a body larger than it still sends whole

## Adaptive windowing

- [x] `http2.adaptiveWindow` opens an HTTP/2 connection at 64 KiB rather than the static default
- [x] `http2.adaptiveWindow` ignores both the common and the HTTP/2 explicit windows
- [ ] `http2.adaptiveWindow` leaves HTTP/3 on whichever windows apply to it
- [ ] An adaptive HTTP/2 window grows over the course of a large transfer

## Flow control still functions

- [x] A body many windows long arrives whole over HTTP/3, so windows are replenished as data is read
- [ ] A body many windows long arrives whole over HTTP/2
- [ ] Concurrent streams on one HTTP/2 connection share the connection window without deadlocking

## Throughput

- [ ] A large transfer over a high-latency link is measurably faster on the 6 MiB default than on a 2 MiB window
- [ ] Memory held by an idle pooled connection does not grow with the configured window size
