# Support configurable alternative DNS transports

## Resolver wiring notes

hickory-resolver 0.26 defaults `ServerOrderingStrategy` to `QueryStatistics`, which reorders the pool by collected latency and "may vary over time".
The spec requires the caller's order to be held fixed, with a failed server retried in its original position, so the resolver must be built with `UserProvidedOrder`.
Leaving the default in place would silently produce the behaviour the spec rules out, and it would not show up in a single-server test.

`ResolverOpts.timeout` is already a deadline across the whole name server pool in 0.26 rather than per server (`name_server_pool.rs`, which computes one deadline for the pool to avoid spending N × timeout), and it defaults to five seconds.
`dns.timeout` therefore maps straight onto it, default included, rather than needing a deadline imposed above the resolver.

The encrypted transports live behind hickory's `__tls`/`__https`/`__quic`/`__h3` features, reached through the `tls-aws-lc-rs`/`https-aws-lc-rs`/`quic-aws-lc-rs`/`h3-aws-lc-rs` public features.
aws-lc-rs is the provider reqwest's `rustls` feature already pulls in, so matching it keeps one crypto provider in the build.
`rustls-platform-verifier` gives hickory the system roots (reqwest uses the same verifier) and, with rustls' `ServerName::IpAddress`, verifies an IP against a certificate's IP SAN.

### Resolved: IP-against-certificate authentication

hickory maps a `ProtocolConfig`'s `server_name: Arc<str>` through `rustls::pki_types::ServerName::try_from`, which yields `ServerName::IpAddress` for an IP string, and `rustls-platform-verifier` checks the IP SAN.
So `tls://1.1.1.1` with the IP as its server name authenticates against the address itself, and no fragment is forced.

### Opportunistic encryption is built in

hickory 0.26 ships RFC 9539 opportunistic encryption (`ResolverBuilder::with_opportunistic_encryption`, in-memory `NameServerTransportState` with optional TOML persistence).
Enabling it without persistence matches the spec's "in memory for the life of the agent; Faith writes no probe state to disk", so the probing tier is a config toggle on the discovery path rather than a subsystem Faith writes.

## Build stages

### Stage 1 — configuration surface and transport wiring (this card)

- [x] Enable hickory encrypted-transport features (`tls`/`https`/`quic`/`h3` aws-lc-rs) and `rustls-platform-verifier` in `Cargo.toml`
- [x] Extend `AgentDnsOptions` with `servers`, `timeout`, `searchDomains`, `ndots`, `hostsFile`, `exemptDomains`
- [x] Parse `dns.servers` URLs into transport/port/path/cert-name specs, throwing `AddressParse` on an unparseable URL or unknown scheme
- [x] Reject `dns.servers` together with `dns.system` as a `Config` error at construction
- [x] Build the resolver from parsed servers with `UserProvidedOrder`, `dns.timeout`, `dns.ndots`, `dns.searchDomains`, and the hosts-file toggle
- [x] Bootstrap hostname-host servers to IPs at first use (IP-host servers first, else system), dropping a server whose hostname will not resolve for the life of the agent
- [x] Route exempt names (`localhost`, `.local`, the system suffix, `dns.exemptDomains`) to the system resolver
- [x] Enable hickory's in-memory opportunistic encryption on the discovery path (unset `dns.servers`)
- [x] Expose `resolvers()` reporting each server's address, transport, and source in query order
- [x] Rust unit tests for URL parsing and exemption matching; JS tests for the config surface and validation

### Not done in this card

Three pieces of the DNS spec are spun out as breakdown entries rather than built here: reading the operating system's encrypted DNS settings, Discovery of Designated Resolvers, and live transport state in `resolvers()`.
See the card breakdown for their scope; this plan does not carry it, since plans are removed at merge and the breakdown is what becomes cards.

Until those land, discovery reads the system's servers and reaches them over conventional DNS or hickory's opportunistic encryption, which is the bottom of the ladder the spec describes, and `resolvers()` reports the servers the resolver was built from.
