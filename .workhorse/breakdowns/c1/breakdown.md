# Read HTTPS/SVCB DNS records as an HTTP/3 hint

This card reads the `alpn` parameter of an `HTTPS` record to learn HTTP/3 before the first connection. The same record can carry other SvcParams that Faith does not yet act on; the ones worth their own cards are spun off below.

## Use the ECH config from the HTTPS record

An `HTTPS` record can carry an `ech` SvcParam holding the origin's Encrypted Client Hello configuration, which would let the TLS handshake encrypt the SNI learnt from the same DNS answer. This is blocked on rustls ECH support settling, so it waits until that lands rather than shipping alongside the `alpn` hint.

## Start connecting from the HTTPS record's address hints

An `HTTPS` record can carry `ipv4hint` and `ipv6hint` SvcParams listing addresses for its target, which RFC 9460 intends a client to start connecting on while the A and AAAA queries are still outstanding. The hints are non-authoritative, so the address queries still run and their answers still govern: a hint that fails to connect falls back to the resolved addresses. The payoff is small under the concurrent lookup this card introduces, since the address answers usually arrive alongside the record, so this is worth measuring against a real resolver before building it.
