---
id: DNS
---

# DNS resolution

Faith resolves names with its own DNS client by default, which is what enables the in-memory DNS cache, Happy Eyeballs, and the encrypted transports.
A caller who wants a particular resolver reached a particular way configures it; a caller who does not gets the system's own resolvers, encrypted wherever Faith can manage it.
The system resolver remains available as an escape hatch for environments where the built-in client misbehaves.

## Built-in resolver

Resolution uses Faith's own client with an in-memory cache; repeat requests to a host skip the lookup entirely (connection reuse skips it further still).
`prefetchDns(host)` populates this cache ahead of the first request (see [WARM](warm-up.md)).
IPv4 and IPv6 answers race with the Happy Eyeballs algorithm, so a broken family degrades latency rather than breaking connectivity.

## Transports

`dns.servers` is an ordered list of resolver URLs, and each URL's scheme selects the transport Faith speaks to that resolver.
`udp://` and `tcp://` are conventional DNS on port 53, `tls://` is DNS over TLS on port 853, `https://` is DNS over HTTPS on port 443, `quic://` is DNS over QUIC on port 853, and `h3://` is DNS over HTTP/3 on port 443.
A port in the URL overrides the transport's conventional port, and the HTTP transports use the `/dns-query` path when the URL supplies none.
A URL with any other scheme, or one that does not parse, throws an address-parse error at agent construction (see [ERR](../errors/errors.md)).

The encrypted transports authenticate the resolver against a name, which the URL supplies in one of two ways.
A URL fragment names the certificate to expect when the host is an IP address, as in `tls://1.1.1.1#cloudflare-dns.com`, matching how systemd-resolved writes the same pairing.
A URL whose host is a hostname authenticates against that hostname, and a fragment overrides it.

Setting `dns.servers` replaces the system's resolver configuration outright, so no discovery runs and only the listed servers are queried.

## Server order

Servers are queried in the order listed, one at a time, and a later server is reached only once the servers before it have failed.
Faith does not reorder the list by observed latency, because the order expresses the caller's intent rather than a performance hint: a list that names a private resolver first and a fallback second must not end up sending most traffic to the fallback for being closer.
A server that fails is retried in its original position on the next lookup rather than being demoted.
The whole list shares one lookup deadline, so exhausting several dead servers costs a single timeout rather than one timeout per server.

## Bootstrapping

A server URL that names a hostname cannot be contacted until that hostname resolves, which the servers that need no bootstrapping resolve on its behalf.
Those are the listed servers whose host is already an IP address, used in list order, so an encrypted server placed above a plaintext one bootstraps its siblings without the hostname being exposed in plaintext.
Where the list has no such server, the system's own resolver configuration bootstraps instead.
Bootstrapping happens when the resolver is first used rather than at agent construction, and a hostname that fails to resolve drops that server from the list for the life of the agent rather than failing construction or resolution.

## Discovery

With `dns.servers` unset, the built-in resolver configures itself from the system, preferring an encrypted transport wherever one is available for the resolvers the system has already chosen.
Discovery never substitutes a different DNS provider: it changes how Faith talks to the system's resolvers, not which resolvers answer, so split-horizon and corporate resolvers keep working.
No public resolver is baked into the library as a discovery target, because choosing a caller's DNS provider for them is not a library's decision to make.

Discovery works down a ladder, taking the first source that yields an encrypted endpoint for a given resolver.
The operating system's own encrypted DNS settings come first, where the platform exposes them to be read.
A resolver that designates an encrypted endpoint of its own comes next, found by asking it directly, following Discovery of Designated Resolvers.
Both of these authenticate the resolver against a name, so either is preferred over probing.
A resolver that neither source covers is queried over conventional DNS, unless opportunistic encryption finds a way to encrypt it.

Discovery is best-effort and never fails a lookup on its own: when an encrypted endpoint cannot be established, resolution proceeds over the transport the system configuration gives.

A host with no readable resolver configuration at all falls back to Google Public DNS over conventional DNS.
This is a last resort that keeps resolution working on a misconfigured host rather than a provider Faith chooses for callers, and it is reached only when the system names no resolver of its own.

## Opportunistic encryption

Where discovery leaves a resolver on conventional DNS, Faith probes it for an encrypted transport it has not advertised, following the unilateral probing described in RFC 9539.
Probes run in the background rather than in the path of a lookup, so a resolver that ignores them costs no latency, and a cap on concurrent probes keeps the background work bounded.
A resolver that answers a probe is used over the encrypted transport from then on, and its plaintext connection is retired rather than reused.
Successful and failed probes are both remembered, for the periods RFC 9539 suggests, so a resolver is neither re-probed constantly nor retried immediately after refusing.
This remembering lives in memory for the life of the agent; Faith writes no probe state to disk.
Closing the agent stops probes still in flight, alongside the resolver itself (see [AGENT](overview.md)).
The transports probed are DNS over TLS and DNS over QUIC, which are the ones a resolver can offer without advertising a path.

Probing does not authenticate the resolver's certificate, which is what the standard requires of it.
This tier therefore protects against passive observation of DNS traffic and not against an attacker able to intercept and alter it, which is why it sits below the two authenticated sources rather than beside them.
For the same reason it is used only when discovery has found no authenticated encrypted endpoint for any resolver: an unauthenticated probe must never weaken the checks made on a resolver reached through the operating system or through Discovery of Designated Resolvers.
Servers listed in `dns.servers` are never probed, since an explicit list is a statement of how the caller wants each resolver reached.

## System resolver

`dns.servers` and discovery configure Faith's built-in resolver only; the system's own resolver is not Faith's to configure.
Setting both `dns.servers` and `dns.system: true` is a contradiction rather than a preference, and throws a configuration error at agent construction (see [ERR](../errors/errors.md)).
`dns.system: true` switches to the system's resolver.
This also disables Happy Eyeballs and the DNS cache; the trade is compatibility for performance, and it is the first thing to try when Faith fails to resolve something other clients can.

## Overrides

`dns.overrides` pins specific domains to specific addresses, taking effect even with `dns.system: true`.
Addresses may carry a port; without one, port 0 means "the conventional port for the protocol in use".
An explicit port in the fetched URL wins over the override's port.
An address that parses as neither `ip:port` nor a bare IP throws an address-parse error at agent construction.
Resolving a domain to an empty address list blocks that domain for the agent: requests to it fail with a network error without any connection being attempted.
