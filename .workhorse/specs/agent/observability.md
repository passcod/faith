---
id: OBS
---

# Agent observability

Agents expose three views of their own activity on both surfaces: cumulative request counters (`stats()`), live per-connection network statistics (`connections()`), and the DNS servers in use (`resolvers()`).
They exist so operational problems (connection leaks, retransmission storms, pool churn, DNS silently falling back to plaintext) can be diagnosed from inside the process, without packet captures.
Each of the three methods carries the same name on both surfaces; the fields it returns are named as concepts here and spelled in each surface's own convention (see [RSAPI](../rust/client-api.md)).

## stats()

`stats()` returns cumulative counters: requests sent, responses received, bodies started, bodies finished, and background requests.
The first four count requests made through the agent rather than exchanges on the wire, so a request served from the HTTP cache counts like any other (see [CACHE](../cache/http-cache.md)).
The bodies-started counter counts bodies opened for reading, which a discarded body is not.
A persistent gap between bodies started and bodies finished is the designed leak indicator for response bodies that were opened but never consumed or discarded.

The background-requests counter counts the requests the agent made on its own initiative rather than ones the caller asked for, which is why they are absent from the other four counters.
It covers the synthetic `HEAD` a `preconnect` sends (see [WARM](warm-up.md)), an eager HTTP/3 probe (see [PROBE](../http3/probing.md)), and a background cache revalidation (see [CACHE](../cache/http-cache.md)).
Counting them together gives an operator the wire traffic the agent generates beyond the caller's own requests, which is otherwise invisible: the caller's counters and the origin's logs disagree by exactly this number.
A background request is counted when it is made, whatever its outcome, since these requests swallow their failures and a counter that moved only on success would hide the case worth seeing.

## connections()

`connections()` lists the agent's current TCP connections with per-connection statistics.
QUIC connections are not tracked; each entry's connection type is TCP.
Each entry identifies the connection by local/remote address and port, and carries usage data: a response count (which may undercount when redirects are followed internally), the times the connection was first and last seen, and an expiry (an estimate of when the connection leaves the pool, pushed back on reuse and derived from the pool idle timeout).
A connection opened by `preconnect(origin)` is listed before any request has used it, with a response count of zero (see [WARM](warm-up.md)).
Network statistics are sampled from the operating system about once a second, so consumers can difference successive readings into rates (e.g. retransmission rate).
An agent with nothing tracked does not sample at all.
The cross-platform fields are the round-trip time and its variance in microseconds, the current and total retransmit counts, and the congestion window.
Lost-packet counts and the delivery rate in bits per second are Linux-only.
Other fields may be missing per platform, and no forward guarantee is made on field availability; on wholly unsupported platforms the list is empty.
Statistics come from the operating system's own TCP introspection, so sampling stays passive against the real kernel state.

## resolvers()

`resolvers()` lists the DNS servers the agent resolves through, so "are my lookups actually encrypted" is answerable from inside the process (see [DNS](dns.md)).
Each entry gives the server's address, the transport in use, and how that transport was arrived at: configured by the caller, read from the operating system's encrypted DNS settings, designated by the resolver itself, established by probing, or conventional DNS.
Entries appear in the order the resolver queries them.
The list reports live state rather than configuration, so an entry's transport changes when a probe succeeds and a server dropped for failing to bootstrap does not appear.
An agent whose resolver has not yet been used lists nothing, because the resolver reads its configuration when first needed.
A network change returns it to that state until the next lookup, the servers it listed having been read off the network that has gone (see [NETCHG](network-change.md)).
`resolvers()` is empty for an agent using the system resolver, which does not report what it does internally.
