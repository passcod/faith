# Evaluate Graviola for TLS implementation

Faith should retain aws-lc-rs as its default Rustls crypto provider.
Graviola is not suitable for Faith’s supported platform and protocol surface at this stage.

## Existing TLS boundary

Faith constructs each agent’s Rustls `ClientConfig` itself, then supplies the configured client to Reqwest.
The configuration combines the selected crypto provider with platform trust verification, explicit extra roots, optional mTLS identity, `SSLKEYLOGFILE`, ALPN, early data, and Faith’s OneCRL and CRLite verifier wrapper.
The configuration also serves both TCP HTTP connections and HTTP/3, with Reqwest changing ALPN for QUIC.

The provider choice is therefore an internal implementation detail at the crypto boundary.
It must not change the Agent API, platform trust semantics, revocation checks, error mapping, or diagnostics.

## Graviola assessment

`rustls-graviola` exposes a Rustls `CryptoProvider`, so it could conceptually replace Faith’s `crypto_provider()` implementation without changing the certificate-verification layer.
Its provider supplies TLS 1.2 and TLS 1.3 cipher suites, X25519, P-256, P-384, and X25519MLKEM768 key exchange, signature verification, signing-key parsing, randomness, and ticket production.

Graviola supports only x86_64 and aarch64.
It requires modern CPU instructions and asserts their availability when cryptographic operations begin.
Faith’s published native builds also include ARMv7 and x86_64 FreeBSD, which Graviola cannot build for.
A provider selected as Faith’s default would consequently remove support from published targets and could crash on older supported x86_64 CPUs.

All three TLS 1.3 cipher suites in `rustls-graviola` declare no QUIC algorithm.
Faith’s default feature set supports HTTP/3 and its TLS configuration is shared with the HTTP/3 client.
Using the provider would therefore make Graviola incompatible with Faith’s HTTP/3 and TLS 1.3 early-data behaviour.

Graviola describes itself as very new and asks users to exercise due caution.
Its Rustls integration is version-compatible in principle with Faith’s Rustls 0.23 dependency, but Faith must not treat that compatibility as sufficient for adoption while platform and QUIC coverage are incomplete.

## Decision

Faith will not add Graviola as a dependency or selectable provider.
The default remains aws-lc-rs, with the existing ring alternative retained for builds that select it.

This decision keeps Faith’s published binary targets, HTTP/3 support, and current mTLS, platform-verification, revocation, key-log, and error behaviours intact.

## Re-evaluation gates

Re-evaluate Graviola only when its Rustls provider supports QUIC for its TLS 1.3 cipher suites and Graviola can cover every Faith release target without requiring unavailable CPU features.

A future evaluation should prove a full feature build against Faith’s resolved Rustls version, exercise HTTP/1.1, HTTP/2, and HTTP/3 handshakes, verify mTLS identities across Faith’s supported PEM key formats, and confirm that platform trust, extra roots, revocation, key logging, and error-code behaviour are unchanged.
