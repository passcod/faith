---
id: AGENT
---

# Agent

An `Agent` is an instance of the HTTP client: it owns a connection pool, a DNS resolver and cache, optional cookie jar and HTTP cache, HTTP/3 knowledge, and statistics.
Every request runs on an agent: reusing connections and DNS answers across requests is where most of Faith's performance comes from, so there is no agentless path.

## The default agent

A `fetch()` without an explicit `agent` uses a module-level default agent, constructed with no options on first use and shared by all such calls for the life of the process.
Creating an agent per request is an anti-pattern the design deliberately does not optimise for: each agent pays for its own resolver, pool, and trust store load.

## Construction

Options are validated at construction.
Errors that indicate a broken configuration throw: an unparseable `localAddress`, DNS override address, or DNS server URL (syntax error), malformed `tls.identity` or `tls.extraRoots` PEM (syntax error), and a disk cache without a path or DNS servers combined with the system resolver (configuration error).
Convenience inputs degrade gracefully instead: default headers with invalid names or values are dropped entry by entry, and the `manual` redirect policy is accepted (see [REDIR](../fetch/redirects.md)).
Node-compatible environment variables are read at construction and layer on top of explicit options (see [ENV](../environment/variables.md)).

## Identity and defaults

`userAgent` sets the `User-Agent` for all requests; the default is `Faith/{version} reqwest/{version}`, and the `USER_AGENT` constant is exported so callers can prepend their own product token.
`headers` sets default headers on every request, marked `sensitive` where appropriate (e.g. `Authorization`); per-request headers override them by name.
`localAddress` forces the source IP for connections.
When unset, on hosts that cannot bind the IPv6 wildcard, the QUIC socket binds the IPv4 wildcard instead (probed once per process), so HTTP/3 works on IPv4-only hosts rather than silently falling back to TCP.
Dual-stack hosts are unaffected.

## Lifecycle

`close()` releases the agent's resources on demand: the connection pool, the DNS resolver, in-flight HTTP/3 probes, and the HTTP/3 knowledge cache.
This exists because waiting for the garbage collector is not acceptable for code that creates many short-lived agents.
Requests already in flight when `close()` is called run to completion; new requests on a closed agent throw a closed-agent error (code `Closed`).
`close()` is idempotent, and the cookie jar remains readable after closing.

`networkChanged()` is the other verb that acts on a live agent's own state, discarding what the agent learned from a network that no longer exists while keeping the agent usable (see [NETCHG](network-change.md)).

## Sub-configuration

Nested option groups are specified in their own areas: [POOL](connection-pool.md), [COOK](cookies.md), [DNS](dns.md), [TLS](tls.md), [OBS](observability.md), [FLOW](flow-control.md), [CACHE](../cache/http-cache.md), [H3UP](../http3/upgrade.md), [QUIC](../http3/transport.md), and [REDIR](../fetch/redirects.md) and [CANCEL](../fetch/cancellation-and-timeouts.md) for the `redirect` and `timeout` groups.
