---
id: TLS
---

# TLS

Trust defaults to the platform's certificate store (Faith bundles no roots of its own), with programmatic and environment-level ways to extend or override it.

## Trust roots

The platform trust store is the default source of roots on every OS.
`tls.extraRoots` adds PEM certificates (as strings or Buffers) to the trust store, for private CAs.
Malformed PEM throws at agent construction: an explicit option is a deliberate act, so it fails loudly (the `NODE_EXTRA_CA_CERTS` equivalent is lenient instead; see [ENV](../environment/variables.md)).

## Client identity

`tls.identity` takes a PEM bundle of private key (RSA, SEC1 EC, or PKCS#8) and at least one certificate, presented as the TLS client certificate (mTLS).
Malformed input throws at construction.

## Connection requirements

`tls.required: true` disables plaintext HTTP for the agent; the default allows it.
`tls.earlyData: true` enables TLS 1.3 0-RTT, sending the first application data with the handshake.
It is off by default because of its security trade-offs (replayability, weaker forward secrecy) and is only effective with HTTP/3.

## Diagnostics

Peer certificate information is always captured and exposed on responses (`response.peer.certificate`), and `SSLKEYLOGFILE` is always honoured (see [ENV](../environment/variables.md)); neither requires configuration.
