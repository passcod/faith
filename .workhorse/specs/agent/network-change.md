---
id: NETCHG
---

# Network-change signal

Node exposes no portable signal for a change of network interface or connectivity, so Faith cannot detect one for the caller.
The agent exposes the reaction instead: `agent.networkChanged()` discards the state a network change can silently invalidate, and the caller wires it to whatever trigger their platform offers (an OS notification, a VPN transition, a captive-portal sign-in), calling it when they know the network underneath the agent has changed.
The point is that the next request rebuilds against the new network rather than reusing answers gathered on the old one.

## What it resets

Calling `agent.networkChanged()` clears, in one call:

- [ ] Pooled connections, so a request after the change reconnects rather than writing into a socket bound to the old network (see [POOL](connection-pool.md)).
- [ ] The DNS cache, so names resolve afresh against the new network; under the system resolver there is no cache to flush and the call does nothing there (see [DNS](dns.md)).
- [ ] The HTTP/3 failed and slow states, together with the consecutive-failure count, the cancellation-strike count, and the cooldown backoff they drive, so an origin whose UDP path was blocked or slow on the old network becomes probe-worthy again rather than staying held down by knowledge the change made stale (see [H3UP](../http3/upgrade.md) and [PROBE](../http3/probing.md)).
- [ ] The per-origin path-time averages for both the QUIC and TCP families, so slow-path demotion judges the new network from fresh samples rather than carrying the old network's timings (see [PROBE](../http3/probing.md)).

HTTP/3 advertised and confirmed knowledge persists across the signal.
An advertisement and a proven upgrade are not invalidated by a network change, and a confirmed origin no longer reachable over its old path heals through the ordinary TCP fallback and re-confirmation (see [H3UP](../http3/upgrade.md)).

## In-flight requests

- [ ] A request already in flight when `networkChanged()` is called runs to completion on its existing connection; the signal never interrupts it.
- [ ] The reset takes effect for work that starts after the call: it reshapes the state the next request draws on, not the state the requests already running depend on.

A caller who needs in-flight requests abandoned on a network change aborts them through their `signal` as usual (see [CANCEL](../fetch/cancellation-and-timeouts.md)).

## Availability

- [ ] `networkChanged()` returns nothing and is safe to call any number of times; calling it with nothing to reset is harmless.
- [ ] On a closed agent it does nothing rather than throwing, since a closed agent holds no pool, resolver, or HTTP/3 knowledge to clear (see [AGENT](overview.md)).
