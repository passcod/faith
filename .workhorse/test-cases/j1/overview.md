# Serving stale DNS entries while revalidating

Scenarios that verify an expired DNS answer is served immediately, refreshed behind the caller, and recovered from when the address has moved.

Automated cases live in `test/agent-dns-stale.test.js`, driving Faith at the controllable nameserver in `test/lib/dns-server.js` via `dns.servers`. `.test` names are used throughout because `localhost` and `.local` are exempt and never reach a configured nameserver.

## Serving and refreshing

- [x] A first lookup of a name pays the resolver's round trip (verifies spec: DNS)
- [x] A lookup of a still-fresh entry answers from cache without reaching the nameserver (verifies spec: DNS)
- [x] A lookup of an expired entry is served immediately rather than waiting for a fresh answer (verifies spec: DNS)
- [x] A stale hit starts a refresh at the nameserver behind the caller (verifies spec: DNS)
- [x] Once the refresh lands, the entry is fresh again and a later lookup needs no query (verifies spec: DNS)
- [x] Five concurrent stale hits are all served, and start at most one refresh between them (verifies spec: DNS)
- [ ] A refresh started by one agent does not serve or refresh another agent's entry

## Refresh outcomes

- [x] A refresh that fails with SERVFAIL leaves the stale entry in place, so a later lookup is still served from it (verifies spec: DNS)
- [x] A refresh whose name has gone (NXDOMAIN) retires the entry, so a later request fails rather than being served a dead address (verifies spec: DNS)
- [ ] A refresh that times out with no answer at all leaves the stale entry in place
- [ ] A refresh returning a different address set replaces the entry, and a later request connects to the new address

## Bounds and options

- [x] An entry older than `dns.maxStale` is discarded, and the lookup blocks on a fresh answer (verifies spec: DNS)
- [x] An entry past `dns.maxStale` is dropped rather than left to age further (verifies spec: DNS)
- [x] `dns.serveStale: false` discards an expired entry and blocks on a fresh answer (verifies spec: DNS)
- [x] `dns.serveStale: false` arms no stale-address retry, nothing having been assumed (verifies spec: DNS)
- [ ] `dns.maxStale` defaults to one hour when unset
- [ ] The stale cache stays bounded under many distinct names rather than growing without limit

## Interaction with the rest of the resolver

- [x] An exempt name (`localhost`) never reaches the configured nameserver, so it is never served stale (verifies spec: DNS)
- [x] `networkChanged()` drops stale answers, so the next lookup resolves against the new network (verifies spec: NETCHG)
- [ ] Under `dns.system: true` no answer is ever stale and neither option has any effect
- [ ] A `dns.overrides` entry is never served stale, an override being consulted ahead of resolution

## When a stale address is wrong

- [x] A `GET` against a stale address that has moved re-resolves and succeeds against the fresh address (verifies spec: DNS)
- [x] A `POST` recovers the same way, the method not bounding this retry (verifies spec: DNS)
- [x] A request carrying a `ReadableStream` body is not attempted again, and the connect failure reaches the caller (verifies spec: DNS)
- [ ] A request that also fails to connect on the fresh address surfaces that failure rather than retrying again
- [ ] A failure after the connection is established (a 5xx, a body that stops early) is returned as it came, with no re-resolve
- [x] A connect failure against an address resolved fresh (past the stale window) arms no re-resolve (verifies spec: DNS)

## Benchmark

- [ ] A features-suite row shows the stale-serve win against a slow resolver (blocked on card W2, which restores the DNS rows and gives the harness a DNS delay knob)
