---
id: DNS
---

# DNS resolution

Faith resolves names with its own DNS client by default, which is what enables the in-memory DNS cache and Happy Eyeballs.
The system resolver remains available as an escape hatch for environments where the built-in client misbehaves.

## Built-in resolver

Resolution uses Faith's own client with an in-memory cache; repeat requests to a host skip the lookup entirely (connection reuse skips it further still).
`prefetchDns(host)` populates this cache ahead of the first request (see [WARM](warm-up.md)).
IPv4 and IPv6 answers race with the Happy Eyeballs algorithm, so a broken family degrades latency rather than breaking connectivity.

## System resolver

`dns.system: true` switches to the system's resolver.
This also disables Happy Eyeballs and the DNS cache; the trade is compatibility for performance, and it is the first thing to try when Faith fails to resolve something other clients can.

## Overrides

`dns.overrides` pins specific domains to specific addresses, taking effect even with `dns.system: true`.
Addresses may carry a port; without one, port 0 means "the conventional port for the protocol in use".
An explicit port in the fetched URL wins over the override's port.
An address that parses as neither `ip:port` nor a bare IP throws an address-parse error at agent construction.
Resolving a domain to an empty address list blocks that domain for the agent: requests to it fail with a network error without any connection being attempted.
