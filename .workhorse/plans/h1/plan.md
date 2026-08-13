# Support configurable alternative DNS transports

## Resolver wiring notes

hickory-resolver 0.26 defaults `ServerOrderingStrategy` to `QueryStatistics`, which reorders the pool by collected latency and "may vary over time".
The spec requires the caller's order to be held fixed, with a failed server retried in its original position, so the resolver must be built with `UserProvidedOrder`.
Leaving the default in place would silently produce the behaviour the spec rules out, and it would not show up in a single-server test.

`ResolverOpts.timeout` is already a deadline across the whole name server pool in 0.26 rather than per server (`name_server_pool.rs`, which computes one deadline for the pool to avoid spending N × timeout), and it defaults to five seconds.
`dns.timeout` therefore maps straight onto it, default included, rather than needing a deadline imposed above the resolver.

## Open

Authenticating a bare-IP resolver against an IP entry in its certificate needs the TLS stack to be driven with an IP server name rather than a hostname.
Confirm the path exists through the DoT, DoQ, and DoH connection builders before relying on it, since a stack that only accepts a DNS name would force the fragment to be mandatory after all.
