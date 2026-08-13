# Configurable alternative DNS transports — test cases

Scenarios verifying the `dns.servers` transport surface, its validation, exempt-name routing, and
`resolvers()` observability. Automated cases live in `test/agent-dns-transports.test.js` and the
`dns::tests` module; manual cases need a reachable resolver and so are not automated here.

## Configuration and validation

- [x] `dns.servers` accepts every transport scheme (`udp`, `tcp`, `tls`, `https`, `quic`, `h3`) at construction — verifies spec: DNS
- [x] A URL's scheme selects the transport and its conventional port (53 / 853 / 443) — verifies spec: DNS
- [x] An explicit port in the URL overrides the conventional one — verifies spec: DNS
- [x] The HTTP transports default the `/dns-query` path when the URL supplies none — verifies spec: DNS
- [x] A fragment names the certificate to authenticate against — verifies spec: DNS
- [x] A bare-IP host with no fragment authenticates against the address; a hostname authenticates against itself — verifies spec: DNS
- [x] An unparseable URL or unknown scheme throws `AddressParse` at construction — verifies spec: ERR
- [x] `dns.servers` combined with `dns.system` throws `Config` at construction — verifies spec: DNS, spec: AGENT

## Exempt names

- [x] `localhost` resolves via the system resolver even when the only configured server is dead — verifies spec: DNS
- [x] An exempt suffix matches a name exactly or as a subdomain — verifies spec: DNS
- [ ] A name under `.local` and under the network's own DNS suffix routes to the system resolver — verifies spec: DNS
- [ ] `dns.exemptDomains` extends the exemption without letting `localhost` reach a public resolver — verifies spec: DNS

## Observability

- [x] `resolvers()` is empty before the resolver is used and for the system resolver — verifies spec: OBS
- [x] `resolvers()` reports configured servers in query order with their address, transport, and `configured` source — verifies spec: OBS

## Resolver wiring (needs a reachable resolver — manual)

- [ ] A working `dns.servers` entry actually resolves a non-exempt name over each transport (DoT, DoH, DoQ, DoH3, udp, tcp)
- [ ] Servers are queried in listed order; a failed server is retried in its original position rather than demoted — verifies spec: DNS
- [ ] `dns.timeout` bounds the whole list, so exhausting several dead servers costs one timeout — verifies spec: DNS
- [ ] A hostname-host server bootstraps via a listed IP-host server, and a hostname that will not resolve drops that server for the agent's life — verifies spec: DNS
- [ ] On the discovery path (no `dns.servers`), a resolver answering a probe is used over the encrypted transport — verifies spec: DNS
