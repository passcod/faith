---
id: ENV
---

# Environment variables

Faith reads a set of environment variables rather than asking a caller to configure what the surrounding platform already answers.
The set is deliberately Node's own vocabulary plus the standard proxy and OpenSSL variables, not a Faith-specific namespace; on the Node surface that is what makes `fetch()` behave like Node's built-in fetch without extra configuration.

Each section below names the surfaces it applies to.
The `NODE_`-prefixed variables answer to a JavaScript runtime's conventions and belong to the Node surface alone; the rest is platform configuration both surfaces honour (see [RUST](../rust/overview.md)).

## Read-at-construction semantics

This applies to both surfaces.

Environment variables are read once, when an agent is constructed, including the Node surface's implicit global default agent.
Changing them afterwards only affects agents created later.

## Trust store

Both surfaces read `SSL_CERT_FILE` and `SSL_CERT_DIR`; `NODE_EXTRA_CA_CERTS` belongs to the Node surface alone.

On Unix platforms other than macOS, `SSL_CERT_FILE` and `SSL_CERT_DIR` override where the system trust store is loaded from, with standard OpenSSL semantics: `SSL_CERT_FILE` replaces the system roots.
On macOS and Windows the OS trust store is used directly and these are ignored, as Node does on those platforms.

`NODE_EXTRA_CA_CERTS` names a PEM file whose certificates are added to the trust store on top of the platform roots and any `tls.extraRoots` (see [TLS](../agent/tls.md)); certificates from both sources combine, and where `SSL_CERT_FILE` replaces the system roots this adds to them.
It is lenient, matching Node's warn-and-continue behaviour: an empty value, an unreadable file, or an unparseable file is ignored rather than fatal.
(The `tls.extraRoots` option, being an explicit programmatic choice, throws on malformed input instead.)
A Rust caller extends the trust store through `tls.extraRoots`.

## Certificate validation

This applies to the Node surface alone.

`NODE_TLS_REJECT_UNAUTHORIZED` set to exactly `0` disables TLS certificate validation for the agent; any other value or unset keeps validation on.
This matches Node's semantics and exists only for that compatibility; trusting a specific private CA via `NODE_EXTRA_CA_CERTS` or `tls.extraRoots` is the supported path.
The Rust surface keeps certificate validation on, that being a compatibility it does not owe.

## Proxies

Both surfaces read the proxy variables and the operating system's own proxy settings; `NODE_USE_ENV_PROXY` belongs to the Node surface alone.

`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY` (and their lowercase spellings) are honoured automatically: per-scheme proxy selection, a fallback for both schemes, and a comma-separated direct-connection list of hosts, domains, and CIDR ranges.
The operating system's proxy settings are also read automatically.

`NODE_USE_ENV_PROXY` set to exactly `0` turns ambient proxy configuration off.
Faith proxies by default, so unlike Node (where the same variable opts in), it acts purely as an opt-out.

## Debugging

This applies to both surfaces.

`SSLKEYLOGFILE` names a path to which TLS session keys are written, enabling decryption of captured traffic when debugging.

## Variables with nothing to control

Neither surface reads these.

`NODE_USE_SYSTEM_CA` is ignored because the platform trust store is Faith's only default source of roots; there is no bundled set to toggle away from.
`OPENSSL_CONF` is ignored because the TLS stack is not OpenSSL, so OpenSSL's configuration file has nothing to configure.
