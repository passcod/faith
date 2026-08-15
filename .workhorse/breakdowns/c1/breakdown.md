# Read HTTPS/SVCB DNS records as an HTTP/3 hint

This card reads the `alpn` parameter of an `HTTPS` record to learn HTTP/3 before the first connection. The same record can carry other SvcParams that Faith does not yet act on; the one worth its own card is spun off below.

## Use the ECH config from the HTTPS record

An `HTTPS` record can carry an `ech` SvcParam holding the origin's Encrypted Client Hello configuration, which would let the TLS handshake encrypt the SNI learnt from the same DNS answer. This is blocked on rustls ECH support settling, so it waits until that lands rather than shipping alongside the `alpn` hint.
