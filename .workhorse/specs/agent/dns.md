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

The encrypted transports always authenticate the resolver, and the URL determines what they authenticate against.
A URL fragment names the certificate to expect, as in `tls://1.1.1.1#cloudflare-dns.com`, matching how systemd-resolved writes the same pairing.
A URL whose host is a hostname authenticates against that hostname, and a fragment overrides it.
A URL whose host is an IP address and carries no fragment authenticates against that address, which requires the resolver's certificate to list the address itself; this is what lets `tls://1.1.1.1` be written without a fragment.
A listed server has no unauthenticated mode: a resolver whose certificate covers neither the hostname nor the address it was reached by fails authentication rather than being reached anyway.

Setting `dns.servers` replaces the system's list of servers, so no discovery runs and lookups go to the listed servers, apart from the names exempt from them entirely (see [Exempt names](#exempt-names)).

## Server order

Servers are queried in the order listed, one at a time, and a later server is reached only once the servers before it have failed.
Faith does not reorder the list by observed latency, because the order expresses the caller's intent rather than a performance hint: a list that names a private resolver first and a fallback second must not end up sending most traffic to the fallback for being closer.
A server that fails is retried in its original position on the next lookup rather than being demoted.
A lookup that reaches the end of the list without an answer fails with a network error (see [ERR](../errors/errors.md)).

`dns.timeout` bounds the whole list rather than each server, so exhausting several dead servers costs a single timeout rather than one timeout per server.
It defaults to five seconds, unlike the agent's request timeouts, which are unset by default (see [CANCEL](../fetch/cancellation-and-timeouts.md)): a lookup with no deadline of its own would hang until whatever bounds the request as a whole.

Falling through to the next server covers a server that does not answer, not one that answers as the wrong identity.
An encrypted server whose certificate fails authentication fails the lookup outright with a network error, rather than counting as a dead server.
The list orders the servers to try; it does not license reaching a named resolver as something else, so a resolver that cannot prove who it is stops the lookup instead of passing the name to whatever comes next.

## Bootstrapping

A server URL that names a hostname cannot be contacted until that hostname resolves.
The listed servers whose host is already an IP address do that resolving, in list order, so an encrypted server placed above a plaintext one bootstraps its siblings without the hostname being exposed in plaintext.
Where the list has no such server, the system's own resolver configuration bootstraps instead.
Bootstrapping happens when the resolver is first used rather than at agent construction, and a hostname that fails to resolve drops that server from the list rather than failing construction or resolution.
A dropped server returns when the resolver is rebuilt, since a hostname that resolves nowhere on one network may resolve on the next (see [Network changes](#network-changes)).

## Preparing a name

The system's search domains, its dots threshold, and its hosts file govern how a name becomes a query, and `dns.servers` leaves all three in place.
The hosts file is consulted before any server, so a name it answers never reaches a resolver at all.

Three options override those inputs independently of `dns.servers`, so how names are prepared can be changed without naming servers, and the reverse.
`dns.searchDomains` replaces the system's search list.
`dns.ndots` sets how many dots a name must contain before it is tried as given, ahead of the search list.
`dns.hostsFile` turns hosts-file lookup on or off, and when unset follows the platform's own convention.

## Exempt names

Some names must not leave the local network, and sending them to a configured or encrypted resolver either leaks them or fails to resolve them.
`localhost`, names under `.local`, and names under the network's own DNS suffix are handed to the system's resolver instead of Faith's servers.
This is also what makes `.local` work at all, since multicast DNS is not something Faith's own client speaks.
The exemption holds whether the servers came from `dns.servers` or from discovery, because its reason is the correctness of local names rather than a preference about transports.

`dns.exemptDomains` adds further domains, for the internal suffixes a network uses that are not its DNS suffix.
It adds to the three above rather than replacing them, so a caller extends the exemption without being able to send `localhost` to a public resolver by accident.
A domain is exempt when it matches an entry exactly or is a subdomain of one.

The root domain is not an exemptable suffix, from any source.
Every name is a subdomain of the root, so treating it as one would exempt all of them and leave a configured `dns.servers` set unused while every lookup went to the system resolver.
A host that has no DNS suffix of its own contributes no suffix here, which is not the same as contributing one that matches everything.

## Discovery

With `dns.servers` unset, the built-in resolver configures itself from the system, preferring an encrypted transport wherever one is available for the resolvers the system has already chosen.
Discovery never substitutes a different DNS provider: it changes how Faith talks to the system's resolvers, not which resolvers answer, so split-horizon and corporate resolvers keep working.
Discovery adds no DNS provider of its own to a host that names one, because choosing a caller's DNS provider for them is not a library's decision to make; a host that names no resolver at all is the one exception, below.

Discovery works down a ladder, taking the first source that yields an encrypted endpoint for a given resolver.
The operating system's own encrypted DNS settings come first, where the platform exposes them to be read: Windows' per-interface DNS-over-HTTPS configuration, and systemd-resolved's DNS-over-TLS settings on Linux, including the server names it pins alongside each address.
Platforms that keep those settings private to the operating system fall through to the sources below, which reach the same resolvers by asking rather than by reading.
A resolver that designates an encrypted endpoint of its own comes next, found by asking it directly, following Discovery of Designated Resolvers.
Both of these authenticate the resolver against a name, so either is preferred over probing.
A resolver that neither source covers is queried over conventional DNS, unless opportunistic encryption finds a way to encrypt it.

Discovery is best-effort and never fails a lookup on its own: when an encrypted endpoint cannot be established, resolution proceeds over the transport the system configuration gives.

A host with no readable resolver configuration at all falls back to Google Public DNS over conventional DNS.
This is a last resort that keeps resolution working on a misconfigured host rather than a provider Faith chooses for callers, and it is reached only when the system names no resolver of its own.
It is a discovered server rather than a listed one, so it is probed for an encrypted transport like any other (see [Opportunistic encryption](#opportunistic-encryption)).

## Opportunistic encryption

Where discovery leaves a resolver on conventional DNS, Faith probes it for an encrypted transport it has not advertised, following the unilateral probing described in RFC 9539.
Probes run in the background rather than in the path of a lookup, so a resolver that ignores them costs no latency, and a cap on concurrent probes keeps the background work bounded.
A resolver that answers a probe is used over the encrypted transport from then on, and its plaintext connection is retired rather than reused.
Successful and failed probes are both remembered, for the periods RFC 9539 suggests, so a resolver is neither re-probed constantly nor retried immediately after refusing.
This remembering lives in memory and lasts as long as the resolver holding it; Faith writes no probe state to disk.
A verdict is about one resolver on one network, so it goes when the resolver is rebuilt (see [Network changes](#network-changes)).
Closing the agent stops probes still in flight, alongside the resolver itself (see [AGENT](overview.md)).
The transports probed are DNS over TLS and DNS over QUIC, which are the ones a resolver can offer without advertising a path.

Probing does not authenticate the resolver's certificate, which is what the standard requires of it.
This tier therefore protects against passive observation of DNS traffic and not against an attacker able to intercept and alter it, which is why it sits below the two authenticated sources rather than beside them.
For the same reason a resolver is probed only when neither authenticated source covers that resolver: an unauthenticated probe must never weaken the checks made on a resolver reached through the operating system or through Discovery of Designated Resolvers.
The gate is per resolver rather than across the list, so a list holding one resolver with an authenticated endpoint and one with none leaves the first as discovery found it and probes the second.
Servers listed in `dns.servers` are never probed, since an explicit list is a statement of how the caller wants each resolver reached.

## Network changes

Most of what the resolver holds is a reading of one network rather than a setting of the agent's, so `networkChanged()` rebuilds the resolver from the caller's options and reads the rest again on the next lookup (see [NETCHG](network-change.md)).
Rebuilt are the servers discovery took from the system, the addresses hostname servers bootstrapped to, the suffixes treated as local, the results of encryption probes, and the cached answers, which go with the resolvers holding them.
Flushing the answers alone would leave the agent looking the same names up again through the previous network's servers, which is the opposite of what the signal is for.

The options the agent was constructed with are untouched, so a list given in `dns.servers` is rebuilt exactly as listed and in the same order.
What changes for such a list is only what had to be learned from the network: a hostname server bootstraps again, so it may reach a different address or return after having been dropped.

Rebuilding happens on the next lookup rather than during the signal, so an agent that never resolves again pays nothing for it.
A lookup already under way completes against the servers it started with, and `resolvers()` reports nothing between the signal and the next lookup, exactly as it does for an agent whose resolver has not been used yet (see [OBS](observability.md)).

## System resolver

`dns.servers`, `dns.timeout`, and discovery configure Faith's built-in resolver only; the system's own resolver is not Faith's to configure, so it keeps its own deadlines and its own choice of transport.
Setting both `dns.servers` and `dns.system: true` is a contradiction rather than a preference, and throws a configuration error at agent construction (see [ERR](../errors/errors.md)).
`dns.system: true` switches to the system's resolver.
This also disables Happy Eyeballs and the DNS cache; the trade is compatibility for performance, and it is the first thing to try when Faith fails to resolve something other clients can.

## Overrides

`dns.overrides` pins specific domains to specific addresses, taking effect even with `dns.system: true`.
An override sits at the top of resolution, consulted before the hosts file and before the exemptions, so a pinned name reaches neither and an empty address list blocks an exempt name as readily as any other.
Addresses may carry a port; without one, port 0 means "the conventional port for the protocol in use".
An explicit port in the fetched URL wins over the override's port.
An address that parses as neither `ip:port` nor a bare IP throws an address-parse error at agent construction.
Resolving a domain to an empty address list blocks that domain for the agent: requests to it fail with a network error without any connection being attempted.
