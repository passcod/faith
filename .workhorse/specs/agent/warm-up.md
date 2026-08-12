---
id: WARM
---

# Preconnect and DNS prefetch

Two agent methods let a caller pay connection-setup costs before the first request needs them, so that request skips the round trips.
`prefetchDns(host)` warms the DNS cache; `preconnect(origin)` resolves the host and opens a pooled connection.
They mirror the browser resource-hint verbs of the same name.
Both are advisory: a later request goes faster when the warm-up succeeded and pays the normal setup cost when it did not, so a warm-up never makes a request fail that would otherwise have worked.
They differ in what they touch: `prefetchDns` reaches only the resolver, while `preconnect` reaches the origin itself with a synthetic request, which the origin can see.

## prefetchDns

`agent.prefetchDns(host)` resolves `host` through the agent's resolver and stores the answer in the in-memory DNS cache, so a later request to that host skips the lookup (see [DNS](dns.md)).
A DNS name has no scheme, port, or path, so those parts are ignored if a fuller string is passed.
Resolution honours `dns.overrides` and races IPv4 and IPv6 under Happy Eyeballs exactly as a request's own lookup does.
With the system resolver (`dns.system: true`) there is no cache to warm, so the call resolves successfully without doing any work.

## preconnect

`agent.preconnect(origin)` resolves the origin's host and opens a connection to it, leaving it idle in the pool so the first request to that origin skips DNS, TCP, and TLS setup (see [POOL](connection-pool.md)).
It takes an origin (`scheme://host[:port]`); a longer URL is reduced to its origin (path, query, fragment, and userinfo stripped, the same reduction the HTTP/3 probe applies), and an omitted port defaults by scheme.
Resolving the host warms the DNS cache as a side effect, so `preconnect` subsumes `prefetchDns` for that host; under the system resolver there is no cache and the lookup only serves the connection.

The connection is established over the transport the next foreground request to that origin would use, so `preconnect` means the same thing whichever protocol the origin lands on:

- a confirmed HTTP/3 origin gets a warm QUIC connection, the same warm start the eager probe leaves behind (see [PROBE](../http3/probing.md));
- every other origin gets a TCP connection, with TLS negotiated for `https` and ALPN deciding HTTP/1 or HTTP/2 so whichever the server picks is what lands warm.

Foreground requests upgrade only from the confirmed state, so a merely-advertised origin is warmed over TCP; warming it triggers a background HTTP/3 probe exactly as a real TCP-routed request to a probe-worthy origin would (see [PROBE](../http3/probing.md)).
The warm-up settles on its TCP connection without waiting for that probe.

## What preconnect sends

A pooled connection is one that has carried a request, so warming the pool means making one: `preconnect` sends a synthetic `HEAD` to the origin's root, the same shape of request the HTTP/3 probe uses, and the origin sees it in its logs.
This is part of what calling `preconnect` means rather than an implementation artefact.
A caller who must not make unsolicited requests to an origin should not preconnect it; there is no option that keeps the warm-up while suppressing the request, because without the request there is no warm connection.
`http3.upgradeProbe: false` does not suppress it either: that option governs HTTP/3 upgrade probing and continues to do only that, so a TCP warm-up still sends its `HEAD` (see [PROBE](../http3/probing.md)).

The response is discarded whatever its status, because the connection rather than the answer is the point, and any status proves the connection as well as a 200 does.
Redirects are not followed: the caller asked for this origin to be warm, and chasing a redirect would spend the warm-up on a different one.
The warm-up neither reads nor writes the HTTP cache, so a cached response cannot stand in for connecting and the discarded response cannot enter the cache (see [CACHE](../cache/http-cache.md)).
It otherwise carries the agent's configuration like any request to that origin, including default headers, `userAgent`, and the cookie jar, so an origin that sets cookies on its root can update the jar from a warm-up (see [COOK](cookies.md)).
The agent's connect and request timeouts bound it, so a silent path fails the warm-up rather than leaving its promise pending forever (see [CANCEL](../fetch/cancellation-and-timeouts.md)).

## Settling and idle lifetime

Both methods return a promise that settles when the warm-up finishes, whether that is the DNS answer landing in the cache or the connection being established and pooled.
Awaiting it sequences a warm-up ahead of the work that benefits from it, which is what lets a readiness check warm several origins before reporting ready and lets a test know a connection is warm before it measures anything; leaving it unawaited is the fire-and-forget usage the browser verbs suggest.
Settling says the attempt finished, not that a usable connection exists: the idle window may already have lapsed, so a request issued after an awaited warm-up can still pay full setup cost.
A warm-up is not a reachability check.

The promise resolves and never rejects, whatever happens on the network.
A DNS failure, a refused connection, a timeout, and an agent closed while the warm-up is in flight all resolve quietly, because the work is advisory and its outcome belongs to the real request rather than to the warm-up.
Fire-and-forget depends on this: a promise that could reject would raise an unhandled rejection in the unawaited call that is the common case.
Caller errors surface differently, thrown synchronously from the call before any asynchronous work begins.
A malformed host or origin throws a parse error, the same class of mistake `dns.overrides` reports for an unparseable address, and a call on a closed agent throws the closed-agent error (code `Closed`) (see [AGENT](overview.md)).

A warm-up connection is an ordinary idle pooled connection: `pool.idleTimeout` closes it when no request claims it in time, and `pool.maxIdlePerHost` bounds it like any other (see [POOL](connection-pool.md)).
That cap is per origin, so warm-ups to different origins never displace one another.
Preconnecting an origin already at its cap closes the newly opened connection and leaves the existing idle ones in place, which costs the caller nothing because that origin is already warm.
A TCP warm-up connection appears in `connections()` with an expiry derived from the idle timeout, like any pooled TCP connection; QUIC warm-ups are not listed there (see [OBS](observability.md)).

A warm-up stays out of the agent's request accounting: the `stats()` counters track requests the caller made through the agent, and a warm-up's own request is not one of those, so neither method moves them (see [OBS](observability.md)).
They coalesce with work already in progress: a `preconnect` for an origin that already holds an idle pooled connection, or a `prefetchDns` for a host already fresh in the cache, does no new work, and concurrent warm-ups for the same target are single-flighted rather than opening duplicates.
