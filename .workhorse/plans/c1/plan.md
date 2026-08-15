# Read HTTPS/SVCB DNS records as an HTTP/3 hint

Query the `HTTPS` record type (RFC 9460) alongside A/AAAA so an origin advertising `alpn="h3"` becomes probe-worthy before the first connection, instead of waiting for an `Alt-Svc` header to arrive over TCP.

## Technical design

**Where the query lives.** In `FaithResolver::lookup` (`src/dns.rs`), not in the middleware. That is what makes it concurrent with the address lookups and what lets `prefetchDns` warm the HTTP/3 knowledge as well as the addresses. hickory 0.26 exposes `resolver.lookup(name, RecordType::HTTPS)`, returning records whose `RData::HTTPS` derefs to `SVCB` with public `svc_priority`, `target_name`, and `svc_params`.

**What the resolver knows.** `reqwest::dns::Resolve` hands over a bare hostname: no scheme, no port. The `HTTPS` record queried at that bare name is by RFC 9460 the record for the origin at the default HTTPS port, so the advertisement is recorded against `https://{host}:443` regardless of which request triggered the lookup. That is what the record actually describes. A non-default port would need the `_<port>._https.` query form, which there is no context here to build.

**Breaking the construction cycle.** `FaithResolver` is built before `AltSvcCache`, and the prober holds a `reqwest::Client` that in turn holds the resolver. So the resolver cannot own either at construction. Instead `dns.rs` defines a sink trait the agent installs afterwards:

- `wants(host) -> bool` gates the query, so an origin already confirmed, failed, or holding a live advertisement costs no DNS traffic
- `record(host, advertisement)` folds the record into the Alt-Svc cache and kicks the probe

The sink lives in `Inner` rather than `Generation`, so `reset()` (network change) leaves it in place: it is wiring, not a reading of the network. `network_changed()` rebuilds the prober, so the sink must be re-installed there or it holds a prober bound to the dropped client.

**Keeping the sink out of the ownership ring.** The prober holds the client, the client holds the resolver, and the resolver holds the sink, so a strong reference from the sink back to the prober closes a cycle and leaks the whole graph — connection pool included — past `Agent::close`, which works by dropping the client. The sink therefore holds the prober weakly; the agent owns the only strong reference. A query whose answer lands after close finds nothing to probe with, which is the right outcome.

**Never delaying the address answer.** The query runs in a spawned task, so a slow or absent `HTTPS` answer cannot hold up connecting. Its outcome belongs to the upgrade layer, not the caller, so failures are swallowed the way a stale refresh's are.

**Record selection.** ServiceMode only (`svc_priority != 0`); AliasMode records point elsewhere and are not followed. Lowest `svc_priority` wins among candidates. A `target_name` that is neither the root (which per RFC 9460 §2.5.2 means the owner name) nor the queried name designates a different host and is not acted on, matching the same-host rule on header advertisements. `port` maps onto the advertised-port machinery already there; absent, it is the origin's own port.

## Checklist

- [x] Add the `HTTPS` record query and its sink to `src/dns.rs`
- [x] Parse the record: ServiceMode, h3 `alpn`, target, `port`, TTL
- [x] Implement the sink over `AltSvcCache` + `H3Prober` in `src/alt_svc.rs`
- [x] Add `wants`-style gating to `AltSvcCache` so a known origin costs no query
- [x] Install the sink in `agent.rs` at construction and on `network_changed`
- [x] Unit tests for record parsing and gating
- [x] Serve `HTTPS` records from the controllable nameserver, with per-type failures
- [x] Agent-level tests: the query is made, gated, and never delays the request
- [x] `cargo fmt`, `cargo clippy`, `cargo test`, then the full suite
