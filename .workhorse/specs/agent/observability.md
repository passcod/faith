---
id: OBS
---

# Agent observability

Agents expose two views of their own activity: cumulative request counters (`stats()`) and live per-connection network statistics (`connections()`). Both exist so operational problems (connection leaks, retransmission storms, pool churn) can be diagnosed from inside the process, without packet captures.

## stats()

- [ ] `stats()` returns cumulative counters: `requestsSent`, `responsesReceived`, `bodiesStarted`, `bodiesFinished`.
- [ ] A persistent gap between `bodiesStarted` and `bodiesFinished` is the designed leak indicator for response bodies that were opened but never consumed or discarded.

## connections()

- [ ] `connections()` lists the agent's current TCP connections with per-connection statistics. QUIC connections are not tracked (an upstream limitation); `connectionType` is `tcp` and would become `quic` if that changes.
- [ ] Each entry identifies the connection by local/remote address and port, and carries usage data: `responseCount` (may undercount when redirects are followed internally), `firstSeen`, `lastSeen`, and `expiry` (an estimate of when the connection leaves the pool, pushed back on reuse and derived from the pool idle timeout).
- [ ] Network statistics are sampled from the operating system about once a second, so consumers can difference successive readings into rates (e.g. retransmission rate). An agent with nothing tracked does not sample at all.
- [ ] Cross-platform fields: `rttUs`, `rttVarUs`, `retransmits`, `totalRetransmits`, `congestionWindow`.
- [ ] `lostPackets` and `deliveryRateBps` are Linux-only. Other fields may be missing per platform, and no forward guarantee is made on field availability; on wholly unsupported platforms the list is empty.
- [ ] Statistics come from the platform's own TCP introspection (netlink socket diagnostics on Linux, per-process socket info on macOS, the TCP table with extended stats on Windows), chosen so sampling stays passive against the real kernel state.
