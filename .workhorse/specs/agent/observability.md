---
id: OBS
---

# Agent observability

Agents expose two views of their own activity: cumulative request counters (`stats()`) and live per-connection network statistics (`connections()`).
Both exist so operational problems (connection leaks, retransmission storms, pool churn) can be diagnosed from inside the process, without packet captures.

## stats()

`stats()` returns cumulative counters: `requestsSent`, `responsesReceived`, `bodiesStarted`, `bodiesFinished`.
They count requests made through the agent rather than exchanges on the wire, so a request served from the HTTP cache counts like any other (see [CACHE](../cache/http-cache.md)).
`bodiesStarted` counts bodies opened for reading, which a discarded body is not.
A persistent gap between `bodiesStarted` and `bodiesFinished` is the designed leak indicator for response bodies that were opened but never consumed or discarded.

## connections()

`connections()` lists the agent's current TCP connections with per-connection statistics.
QUIC connections are not tracked; each entry's `connectionType` is `tcp`.
Each entry identifies the connection by local/remote address and port, and carries usage data: `responseCount` (may undercount when redirects are followed internally), `firstSeen`, `lastSeen`, and `expiry` (an estimate of when the connection leaves the pool, pushed back on reuse and derived from the pool idle timeout).
Network statistics are sampled from the operating system about once a second, so consumers can difference successive readings into rates (e.g. retransmission rate).
An agent with nothing tracked does not sample at all.
Cross-platform fields: `rttUs`, `rttVarUs`, `retransmits`, `totalRetransmits`, `congestionWindow`.
`lostPackets` and `deliveryRateBps` are Linux-only.
Other fields may be missing per platform, and no forward guarantee is made on field availability; on wholly unsupported platforms the list is empty.
Statistics come from the operating system's own TCP introspection, so sampling stays passive against the real kernel state.
