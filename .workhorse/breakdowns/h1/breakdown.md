# Support configurable alternative DNS transports

Work spun out of this card rather than done in it.

## Validate DNSSEC on the built-in resolver · S2

Hickory can validate DNSSEC signatures on lookups, behind a crate feature Faith does not currently enable, configured per resolver with a trust anchor. It is resolver configuration in the same family as the transports, but no browser validates DNSSEC, so it is a deliberate step beyond browser parity rather than part of reaching it. Wants its own decisions about what a validation failure does to a fetch, where trust anchors come from, and whether validation is worth the added latency by default.

## Read the operating system's encrypted DNS settings · T2

The DNS spec's discovery ladder puts the platform's own encrypted DNS configuration at the top: Windows' per-interface DNS-over-HTTPS settings, and systemd-resolved's DNS-over-TLS settings on Linux including the server names it pins alongside each address. Hickory reads neither, so each is platform-specific work against a different interface, guarded per platform and only testable on that platform. Faith's transports and server ordering already carry the endpoints such a source would produce, so this card is about learning what the operating system has chosen rather than about how to reach it.

## Discover designated resolvers · U2

A resolver can designate an encrypted endpoint of its own, found by querying it directly per Discovery of Designated Resolvers (RFC 9462). It sits on the discovery ladder below the operating system's settings and above unauthenticated probing, and it authenticates the resolver against a name, which is what earns it that position. Wants its own work because it is a DNS query and answer parse rather than configuration reading, with decisions about when the discovery query runs relative to the first lookup and how long a designation is trusted.

## Report live resolver transport state · V2

`resolvers()` reports the servers an agent resolves through, and the spec has an entry's transport change when an opportunistic probe upgrades that server. Reading that live state means surfacing hickory's own per-server transport state at the time of the call rather than the configuration the resolver was built from. Small next to the discovery sources, but it is what makes the method answer "are my lookups encrypted right now" rather than "what were they set up as".
