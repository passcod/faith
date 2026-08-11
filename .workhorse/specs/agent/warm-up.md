---
id: WARM
---

# Preconnect and DNS prefetch

Two agent methods let a caller pay connection-setup costs before the first request needs them, so that request skips the round trips.
`prefetchDns(host)` warms the DNS cache; `preconnect(origin)` resolves the host and opens a pooled connection.
They mirror the browser resource-hint verbs of the same name.
Both are advisory: a later request goes faster when the warm-up succeeded and pays the normal setup cost when it did not, so a warm-up never makes a request fail that would otherwise have worked.

## prefetchDns

`agent.prefetchDns(host)` resolves `host` through the agent's resolver and stores the answer in the in-memory DNS cache, so a later request to that host skips the lookup (see [DNS](dns.md)).
A DNS name has no scheme, port, or path, so those parts are ignored if a fuller string is passed.
Resolution honours `dns.overrides` and races IPv4 and IPv6 under Happy Eyeballs exactly as a request's own lookup does.
With the system resolver (`dns.system: true`) there is no cache to warm, so the call is a no-op.
A host already fresh in the cache is not resolved again.

## preconnect

`agent.preconnect(origin)` resolves the origin's host and opens a connection to it, leaving it idle in the pool so the first request to that origin skips DNS, TCP, and TLS setup (see [POOL](connection-pool.md)).
It takes an origin (`scheme://host[:port]`); a longer URL is reduced to its origin (path, query, fragment, and userinfo stripped, the same reduction the HTTP/3 probe applies), and an omitted port defaults by scheme.
Resolving the host warms the DNS cache as a side effect, so `preconnect` subsumes `prefetchDns` for that host.

The connection is established over the transport the next foreground request to that origin would use, so `preconnect` means the same thing whichever protocol the origin lands on:

- a confirmed HTTP/3 origin gets a warm QUIC connection, the same warm start the eager probe leaves behind (see [PROBE](../http3/probing.md));
- every other origin gets a TCP connection, with TLS negotiated for `https` and ALPN deciding HTTP/1 or HTTP/2 so whichever the server picks is what lands warm.

Foreground requests upgrade only from the confirmed state, so a merely-advertised origin is warmed over TCP; warming it triggers a background HTTP/3 probe exactly as a real TCP-routed request to a probe-worthy origin would (see [PROBE](../http3/probing.md)).

## Settling and idle lifetime

Both methods return a promise that settles when the warm-up finishes — the DNS answer is cached, or the connection is established and pooled — so a caller can await readiness or leave the promise unawaited and fire and forget.
Neither rejects on a DNS or network failure: the work is advisory, and the outcome resurfaces on the real request rather than on the warm-up.
A malformed host or origin rejects the promise with a parse error, the same class of mistake `dns.overrides` reports for an unparseable address, because it is a caller error rather than a transient condition.

A warm-up connection is an ordinary idle pooled connection: `pool.idleTimeout` closes it when no request claims it in time, and `pool.maxIdlePerHost` caps and evicts it like any other, so preconnecting more origins than the cap allows keeps only the most recent (see [POOL](connection-pool.md)).
Preconnecting far ahead of the first request buys nothing once the idle window lapses.
A TCP warm-up connection appears in `connections()` with an expiry derived from the idle timeout, like any pooled TCP connection; QUIC warm-ups are not listed there (see [OBS](observability.md)).

Warm-ups are not requests: they leave the `stats()` request counters untouched and neither read nor write the HTTP cache (see [OBS](observability.md), [CACHE](../cache/http-cache.md)).
They coalesce with work already in progress: a `preconnect` for an origin that already holds an idle pooled connection, or a `prefetchDns` for a host already fresh in the cache, does no new work, and concurrent warm-ups for the same target are single-flighted rather than opening duplicates.
On a closed agent both fail with the closed-agent error (code `Closed`), like a request (see [AGENT](overview.md)).
