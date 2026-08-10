---
id: ENV
---

# Environment variables

Fáith reads a set of environment variables so that `fetch()` behaves like Node's built-in fetch without extra configuration.
The set is deliberately Node's own vocabulary plus the standard proxy and OpenSSL variables, not a Fáith-specific namespace.

## Read-at-construction semantics

Environment variables are read once, when an `Agent` is constructed (including the implicit global default agent).
Changing them afterwards only affects agents created later.

## Trust store

`NODE_EXTRA_CA_CERTS` names a PEM file whose certificates are added to the trust store on top of the platform roots and any `tls.extraRoots` (see [TLS](../agent/tls.md)); certificates from both sources combine.
It is lenient, matching Node's warn-and-continue behaviour: an empty value, an unreadable file, or an unparseable file is ignored rather than fatal.
(The `tls.extraRoots` option, being an explicit programmatic choice, throws on malformed input instead.)
On Unix platforms other than macOS, `SSL_CERT_FILE` and `SSL_CERT_DIR` override where the system trust store is loaded from, with standard OpenSSL semantics: `SSL_CERT_FILE` replaces the system roots, where `NODE_EXTRA_CA_CERTS` adds to them.
On macOS and Windows the OS trust store is used directly and these are ignored, as Node does on those platforms.

## Certificate validation

`NODE_TLS_REJECT_UNAUTHORIZED` set to exactly `0` disables TLS certificate validation for the agent; any other value or unset keeps validation on.
This matches Node's semantics and exists only for that compatibility; trusting a specific private CA via `NODE_EXTRA_CA_CERTS` or `tls.extraRoots` is the supported path.

## Proxies

`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY` (and their lowercase spellings) are honoured automatically: per-scheme proxy selection, a fallback for both schemes, and a comma-separated direct-connection list of hosts, domains, and CIDR ranges.
The operating system's proxy settings are also read automatically.
`NODE_USE_ENV_PROXY` set to exactly `0` turns ambient proxy configuration off.
Fáith proxies by default, so unlike Node (where the same variable opts in), it acts purely as an opt-out.

## Debugging

`SSLKEYLOGFILE` names a path to which TLS session keys are written, enabling decryption of captured traffic in tools like Wireshark.

## Variables with nothing to control

`NODE_USE_SYSTEM_CA` is ignored because the platform trust store is Fáith's only default source of roots; there is no bundled set to toggle away from.
`OPENSSL_CONF` is ignored because the TLS stack is not OpenSSL, so OpenSSL's configuration file has nothing to configure.
