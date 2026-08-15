# Read HTTPS/SVCB DNS records as an HTTP/3 hint

Scenarios verifying that an `HTTPS` DNS record advertising `alpn="h3"` makes an origin probe-worthy before anything has connected to it, and that adding the query costs the request nothing.

## Reading the record

- [x] A ServiceMode record whose `alpn` lists `h3` yields an advertisement (verifies spec: H3UP)
- [x] A draft token such as `h3-29` counts as h3, matching the `Alt-Svc` reader (verifies spec: H3UP)
- [x] A record whose `alpn` lists only `h2` yields nothing (verifies spec: H3UP)
- [x] A record with no target names the origin itself and is acted on (verifies spec: H3UP)
- [x] A record naming the queried host explicitly is acted on, ignoring case and the trailing root (verifies spec: H3UP)
- [x] A record targeting a different host is not acted on (verifies spec: H3UP)
- [x] An AliasMode record is skipped (verifies spec: H3UP)
- [x] Among several ServiceMode records the lowest priority value wins (verifies spec: H3UP)
- [x] An answer with no records yields nothing (verifies spec: H3UP)
- [x] The record's `port` parameter is carried through (verifies spec: H3UP)

## Folding into origin knowledge

- [x] A record lands as advertised, not confirmed, so no foreground request routes on it (verifies spec: H3UP)
- [x] A record naming no port describes the origin's own port (verifies spec: H3UP)
- [x] A record's port differing from the origin's is not acted on by default, and is acted on under `quirks.h3FollowAdvertisedPort` (verifies spec: H3UP#advertised-ports)
- [x] A record is refused while the origin is inside a failure cooldown (verifies spec: H3UP)
- [x] The advertisement expires at the record's own DNS TTL (verifies spec: H3UP)
- [ ] An advertisement from a record is confirmed by the background probe and the next request upgrades (verifies spec: PROBE)
- [ ] A record for an origin whose QUIC path is blackholed costs no foreground latency (verifies spec: PROBE)

## Gating the query

- [x] An origin nothing is known about is queried (verifies spec: DNS#https-records)
- [x] An origin already holding a live advertisement is not queried again (verifies spec: DNS#https-records)
- [x] A confirmed origin is not queried (verifies spec: DNS#https-records)
- [x] A failed origin is not queried (verifies spec: DNS#https-records)
- [x] The query resumes once an advertisement lapses without being confirmed (verifies spec: DNS#https-records)
- [x] No query is made with HTTP/3 upgrade disabled (verifies spec: DNS#https-records)
- [ ] No query is made under `dns.system: true` (verifies spec: DNS#https-records)
- [ ] No query is made for an exempt name (verifies spec: DNS#exempt-names)

## Not disturbing resolution

- [x] The query accompanies the address lookup and the request succeeds (verifies spec: DNS#https-records)
- [x] A name with no `HTTPS` record resolves and fetches unaffected (verifies spec: DNS#https-records)
- [x] A dropped `HTTPS` query does not delay the request whose lookup triggered it (verifies spec: DNS#https-records)
- [ ] A `SERVFAIL` on the `HTTPS` query leaves the address answer and the request untouched (verifies spec: DNS#https-records)
- [ ] A network change leaves the query working, against the new network's resolvers (verifies spec: NETCHG)
- [ ] Closing an agent still releases its resolver and connection pool with the record reading wired up, and an answer arriving after close probes nothing (verifies spec: AGENT)

## Test infrastructure

- [x] The controllable nameserver encodes an `HTTPS` record the way RFC 9460 spells one, decoded independently of Faith
- [x] A name with addresses but no `HTTPS` record answers NOERROR with no records
