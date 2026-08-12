# Preconnect and DNS prefetch

Coverage for the two warm-up verbs. The behaviour is specified in [WARM](../../specs/agent/warm-up.md).

Automated cases live in `test/agent-warm-up.test.js` (the TCP path and the promise contract),
`test/http3-warm-up.test.js` (transport routing, needs Caddy and Linux), and the `agent::tests`
module in `src/agent.rs` (origin and host parsing).

## preconnect: the warm connection

- [x] A warm-up connection is listed in `connections()` before any request uses it, at a response count of zero (verifies spec: WARM, OBS)
- [x] The next request to that origin lands on the warm connection rather than dialling another (verifies spec: WARM, POOL)
- [x] A request that lands on a warm-up connection pays no connection setup or lookup (verifies spec: WARM, RESP)
- [x] A QUIC warm-up is not listed among the TCP connections (verifies spec: WARM, OBS)
- [ ] A warm-up connection is closed by `pool.idleTimeout` when no request claims it in time (verifies spec: WARM)
- [ ] Preconnecting an origin already at `pool.maxIdlePerHost` closes the newly opened connection and leaves the existing idle ones in place (verifies spec: WARM)
- [x] Warm-ups to different origins do not displace one another under a per-origin cap (verifies spec: WARM, POOL)

## preconnect: transport routing

- [x] A confirmed HTTP/3 origin is warmed over QUIC, and the following request runs over it (verifies spec: WARM, PROBE)
- [x] An unconfirmed origin is warmed over TCP (verifies spec: WARM)
- [x] With `http3.upgradeEnabled: false` a hinted origin is still warmed over TCP (verifies spec: WARM)
- [x] With `http3.upgradeProbe: false` a hinted origin is warmed over QUIC, matching the inline upgrade (verifies spec: WARM)
- [x] A TCP warm-up settles without waiting on any HTTP/3 probe it triggered (verifies spec: WARM, PROBE)
- [ ] Warming a probe-worthy origin over TCP triggers a background HTTP/3 probe, observed by the origin becoming confirmed afterwards (verifies spec: WARM, PROBE)
- [ ] `http3.upgradeProbe: false` does not suppress the TCP warm-up's `HEAD` (verifies spec: WARM)

## preconnect: what it sends

- [x] The origin sees a `HEAD` to its root (verifies spec: WARM)
- [x] The warm-up carries the agent's `userAgent` and default headers (verifies spec: WARM)
- [ ] An origin that sets a cookie on its root updates the jar from a warm-up (verifies spec: WARM, COOK)
- [ ] The warm-up neither reads nor writes the HTTP cache: a cached response cannot stand in for connecting, and the discarded response does not enter the cache (verifies spec: WARM, CACHE)
- [ ] The agent's connect and request timeouts bound a warm-up to a silent path, rather than leaving its promise pending (verifies spec: WARM, CANCEL)
- [ ] A warm-up whose root redirects still connects to and pools the origin asked for (verifies spec: WARM)

## The promise contract

- [x] A refused connection resolves quietly (verifies spec: WARM)
- [x] A DNS failure resolves quietly (verifies spec: WARM)
- [x] An agent closed mid-flight resolves the warm-up quietly (verifies spec: WARM)
- [x] A malformed origin or host throws `AddressParse` synchronously, as a `SyntaxError` (verifies spec: WARM)
- [x] A call on a closed agent throws `Closed` synchronously, as a `TypeError` (verifies spec: WARM, AGENT)
- [x] Neither method moves the `stats()` counters (verifies spec: WARM, OBS)

## Coalescing

- [x] Concurrent `preconnect` calls for one origin are single-flighted rather than opening duplicates (verifies spec: WARM)
- [x] A `preconnect` for an origin that already holds an idle pooled connection does no new work (verifies spec: WARM)
- [x] That holds for an origin ordinary traffic warmed, not just one a previous warm-up did (verifies spec: WARM)
- [x] Path, query, fragment, and userinfo variants of one origin reduce to the same origin (verifies spec: WARM)
- [x] An omitted port defaults by scheme, so an origin spelled either way coalesces (verifies spec: WARM)
- [x] Distinct scheme, host, or port are kept apart as distinct origins (verifies spec: WARM, POOL)
- [ ] A `prefetchDns` for a host already fresh in the cache does no new work (verifies spec: WARM)

## prefetchDns

- [x] A bare host resolves and warms the cache a later request reads (verifies spec: WARM, DNS)
- [x] A scheme, port, or path in a fuller string is ignored rather than rejected (verifies spec: WARM)
- [x] An IPv6 literal is unwrapped from its brackets for the resolver (verifies spec: WARM)
- [x] Resolution honours `dns.overrides` (verifies spec: WARM, DNS)
- [x] Under `dns.system: true` the call resolves without doing any work (verifies spec: WARM, DNS)
- [ ] Resolution races IPv4 and IPv6 under Happy Eyeballs as a request's own lookup does (verifies spec: WARM, DNS)

## Interaction with the dead-connection retry

- [x] A `GET` recovers when the origin abandoned the warm-up's connection (verifies spec: WARM, POOL)
- [x] A `POST` onto a dead warm-up connection never comes back with the wrong answer (verifies spec: WARM, POOL)
- [ ] A `POST` onto a dead warm-up connection surfaces the failure to the caller rather than being replayed (verifies spec: WARM, POOL)

The last case is a race that cannot be forced on loopback, the same limitation the `aggressive idle
close` conformance dimension documents: the automated test asserts the answer is never wrong and
accepts either outcome, so the not-replayed half is not yet pinned.

## Regression surface

Fáith now owns the DNS resolver for every request, not just warm-ups, so the existing DNS coverage
is part of this card's surface.

- [x] The full `test/agent-dns.test.js` suite passes against Fáith's own resolver
- [x] The full JS and Rust suites pass
- [ ] The conformance matrix is unchanged
