---
id: PROBE
---

# Eager HTTP/3 probing

An `Alt-Svc` advertisement says the server listens on UDP; it cannot say there is UDP connectivity between this client and that server.
Verifying that inline would sacrifice a foreground request to a possible 30-60 second stall on a silently broken path, once per failure TTL, forever.
So advertisements only make an origin probe-worthy: a background probe verifies the path, and foreground requests keep to TCP until it has.

## The probe

A probe is a `HEAD` request to the origin's root (query, fragment, and userinfo stripped), forced to HTTP/3, launched in the background when an advertisement arrives on a non-HTTP/3 response, and again opportunistically at the start of any TCP-routed request whose origin is probe-worthy.
Three properties of the probe are load-bearing: it neither reads nor writes the HTTP cache, so a replayed HTTP/3-versioned response cannot fake a confirmation; it cannot itself trigger further probing; and its QUIC connection joins the agent's connection pool, so the first upgraded request starts warm.
Any HTTP/3 response confirms the origin, whatever its status: a 401 or 405 proves the transport as well as a 200 does.
Anything else (a non-HTTP/3 response, an error, or the probe timeout, `http3.upgradeProbeTimeout`, default 5 seconds, 0 for unbounded) records a failure with the usual failure TTL.
Probes are single-flight per origin: concurrent triggers for the same origin produce one probe.
Origins already confirmed, failed, or marked slow are not probe-worthy.
Open probes are aborted by `Agent.close()`.
`http3.upgradeProbe: false` restores the inline upgrade (foreground requests attempt HTTP/3 straight from an advertisement), for operators who cannot tolerate synthetic requests.

## Slow-path demotion

Beyond broken-vs-working, Fáith tracks how fast each path actually is: a smoothed average of time-to-response-headers per protocol family (QUIC vs TCP) per origin.
An origin whose QUIC path is sustainedly slower than its TCP path is demoted from confirmed back to advertised: slower means the average exceeds the TCP average by the `http3.upgradeSlowFactor` multiple (default 2.5) and by an absolute floor of about 10ms, with at least 8 samples on each side, so noise and sub-millisecond differences never demote.
Setting the factor to 0 or less disables demotion.
A slow origin is not a broken one: it keeps its advertisement and re-enters through a background probe after `http3.upgradeSlowTtl` (default 10 minutes), so asking "has the path improved?" never costs a foreground request either.
