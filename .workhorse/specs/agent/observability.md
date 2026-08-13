---
id: OBS
---

# Agent observability

Agents expose three views of their own activity: cumulative request counters (`stats()`), live per-connection network statistics (`connections()`), and the DNS servers in use (`resolvers()`).
They exist so operational problems (connection leaks, retransmission storms, pool churn, DNS silently falling back to plaintext) can be diagnosed from inside the process, without packet captures.

## stats()

`stats()` returns cumulative counters: `requestsSent`, `responsesReceived`, `bodiesStarted`, `bodiesFinished`, and `backgroundRequests`.
The first four count requests made through the agent rather than exchanges on the wire, so a request served from the HTTP cache counts like any other (see [CACHE](../cache/http-cache.md)).
`bodiesStarted` counts bodies opened for reading, which a discarded body is not.
A persistent gap between `bodiesStarted` and `bodiesFinished` is the designed leak indicator for response bodies that were opened but never consumed or discarded.

`backgroundRequests` counts the requests the agent made on its own initiative rather than ones the caller asked for, which is why they are absent from the other four counters.
It covers the synthetic `HEAD` a `preconnect` sends (see [WARM](warm-up.md)), an eager HTTP/3 probe (see [PROBE](../http3/probing.md)), and a background cache revalidation (see [CACHE](../cache/http-cache.md)).
Counting them together gives an operator the wire traffic the agent generates beyond the caller's own requests, which is otherwise invisible: the caller's counters and the origin's logs disagree by exactly this number.
A background request is counted when it is made, whatever its outcome, since these requests swallow their failures and a counter that moved only on success would hide the case worth seeing.

## connections()

`connections()` lists the agent's current TCP connections with per-connection statistics.
QUIC connections are not tracked; each entry's `connectionType` is `tcp`.
Each entry identifies the connection by local/remote address and port, and carries usage data: `responseCount` (may undercount when redirects are followed internally), `firstSeen`, `lastSeen`, and `expiry` (an estimate of when the connection leaves the pool, pushed back on reuse and derived from the pool idle timeout).
A connection opened by `preconnect(origin)` is listed before any request has used it, with a `responseCount` of zero (see [WARM](warm-up.md)).
Network statistics are sampled from the operating system about once a second, so consumers can difference successive readings into rates (e.g. retransmission rate).
An agent with nothing tracked does not sample at all.
Cross-platform fields: `rttUs`, `rttVarUs`, `retransmits`, `totalRetransmits`, `congestionWindow`.
`lostPackets` and `deliveryRateBps` are Linux-only.
Other fields may be missing per platform, and no forward guarantee is made on field availability; on wholly unsupported platforms the list is empty.
Statistics come from the operating system's own TCP introspection, so sampling stays passive against the real kernel state.

## resolvers()

`resolvers()` lists the DNS servers the agent resolves through, so "are my lookups actually encrypted" is answerable from inside the process (see [DNS](dns.md)).
Each entry gives the server's address, the transport in use, and how that transport was arrived at: configured by the caller, read from the operating system's encrypted DNS settings, designated by the resolver itself, established by probing, or conventional DNS.
Entries appear in the order the resolver queries them.
The list reports live state rather than configuration, so an entry's transport changes when a probe succeeds and a server dropped for failing to bootstrap does not appear.
An agent whose resolver has not yet been used lists nothing, because the resolver reads its configuration when first needed.
`resolvers()` is empty for an agent using the system resolver, which does not report what it does internally.
