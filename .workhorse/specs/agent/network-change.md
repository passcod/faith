---
id: NETCHG
---

# Network-change signal

Node exposes no portable signal for a change of network interface or connectivity, so Faith cannot detect one for the caller.
The agent exposes the reaction instead: `agent.networkChanged()` discards the state a network change invalidates, and the caller wires it to whatever trigger their platform offers (an OS notification, a VPN transition, a captive-portal sign-in), calling it when they know the network underneath the agent has changed.
The point is that the next request rebuilds against the new network rather than reusing answers gathered on the old one.

## What the signal means

The signal draws one line through everything an agent holds, and that line governs any state the agent gains later without the state having to be named here.

- [ ] State the agent learned by observing the network is discarded. A network change is precisely the event that invalidates it: it was true of a path that no longer exists, and holding it means judging the new network by the old one's behaviour. Reachability findings, timing measurements, resolved addresses, open sockets, and the counters and cooldowns derived from them are all of this kind.
- [ ] State the agent was given is kept. Configuration, the caller's own assertions, and what an origin told Faith about itself are not claims about a network path, so a change of path says nothing about them.
- [ ] Discarding is a reset to the unknown, not a reset to a failure. Cleared state leaves each origin as though the agent had just been constructed and had never reached it, so the next request rediscovers it at full cost and nothing is held down by a penalty the old network earned.

A subsystem added later inherits this rule: its network-derived state is cleared by the signal, and whether that happens is not an open question per subsystem.

## Reach across the subsystems

The rule above resolves, for the state an agent holds today, to:

- [ ] Pooled connections are discarded, so a request after the change reconnects rather than writing into a socket bound to the old network (see [POOL](connection-pool.md)).
- [ ] The DNS cache is flushed, so names resolve afresh against the new network. Under the system resolver there is no cache to flush and the signal does no DNS work (see [DNS](dns.md)).
- [ ] Every HTTP/3 origin proven by observation drops from confirmed to advertised, keeping its advertisement and taking a fresh advertised lifetime. The path that proved it is gone, so it is re-verified rather than trusted: a background probe re-confirms it without a foreground request paying for the check (see [H3UP](../http3/upgrade.md) and [PROBE](../http3/probing.md)).
- [ ] The HTTP/3 failed and slow states clear, along with the consecutive-failure count, the cancellation-strike count, and the cooldown backoff they drive, so an origin whose UDP path was blocked or slow on the old network is probe-worthy again immediately (see [H3UP](../http3/upgrade.md)).
- [ ] The per-origin path-time averages clear for both the QUIC and TCP families, so slow-path demotion judges the new network from fresh samples (see [PROBE](../http3/probing.md)).

The reset leaves every HTTP/3 origin either advertised or unknown, so the demotions and the clearances together mean the first request to an origin after the signal routes over TCP and the re-verification runs behind it.

## What the signal keeps

- [ ] Agent configuration is untouched: the options the agent was constructed with continue to govern it.
- [ ] `http3.hints` keep their origins confirmed. A hint is the caller's assertion that an origin speaks HTTP/3 rather than something Faith observed, so it survives the signal and is still never probed, which is what keeps HTTP/3-only origins reachable across a network change (see [H3UP](../http3/upgrade.md)).
- [ ] `Alt-Svc` advertisements are kept. An advertisement is the origin's statement about itself, and a change of client network does not revise it.
- [ ] The cookie jar and the HTTP cache are kept, holding content and origin state rather than anything about a network path (see [COOK](cookies.md) and [CACHE](../cache/http-cache.md)).
- [ ] The `stats()` counters are kept, being a cumulative record of what the agent has done rather than a description of the network (see [OBS](observability.md)).

## In-flight requests

- [ ] A request already in flight when `networkChanged()` is called runs to completion on its existing connection; the signal never interrupts it.
- [ ] The reset takes effect for work that starts after the call: it reshapes the state the next request draws on, not the state the requests already running depend on.

A caller who needs in-flight requests abandoned on a network change aborts them through their `signal` as usual (see [CANCEL](../fetch/cancellation-and-timeouts.md)).
An in-flight request that outlives the signal can still record what it learns when it completes, so a confirmation or a failure landing just after a reset describes the new network as far as the agent can tell.

## Availability

- [ ] `networkChanged()` returns nothing and is safe to call any number of times; calling it with nothing to reset is harmless, so a caller with a noisy trigger does not need to filter it.
- [ ] On a closed agent it does nothing rather than throwing, a closed agent having already released the state the signal clears (see [AGENT](overview.md)).
