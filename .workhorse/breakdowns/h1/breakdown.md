# Support configurable alternative DNS transports

Work spun out of this card rather than done in it.

## Validate DNSSEC on the built-in resolver

Hickory can validate DNSSEC signatures on lookups, behind a crate feature Faith does not currently enable, configured per resolver with a trust anchor. It is resolver configuration in the same family as the transports, but no browser validates DNSSEC, so it is a deliberate step beyond browser parity rather than part of reaching it. Wants its own decisions about what a validation failure does to a fetch, where trust anchors come from, and whether validation is worth the added latency by default.
