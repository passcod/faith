---
id: H3UP
---

# HTTP/3 upgrade

HTTP/3 cannot be negotiated on an existing connection: a server advertises it via the `Alt-Svc` header, and the client chooses to try UDP.
Fáith keeps per-origin knowledge of those advertisements and their outcomes, so requests upgrade when HTTP/3 is genuinely reachable and never hang on paths where it is not.
The whole mechanism is on by default and disabled entirely with `http3.upgradeEnabled: false`.

## Origin knowledge

Knowledge is keyed by origin (`scheme://host:port`, always the origin's own port) and held in a bounded cache (`upgradeCacheCapacity`, default 10,000 origins) so an agent touching many origins stays bounded in memory.
An origin is in one of three states, each with its own lifetime.
**Advertised** means the server said HTTP/3 exists; it lives for `upgradeAdvertisedTtl` (default 1 day), or the advertisement's own `ma` when given, and with probing on, an advertisement alone never routes a foreground request (see [PROBE](probing.md)).
**Confirmed** means HTTP/3 was proven end to end by an actual HTTP/3 response; it lives for `upgradeConfirmedTtl` (default 1 day) and is the only state foreground requests upgrade from.
**Failed** means an attempt failed; it blocks upgrading, probing, and even recording fresh advertisements, so a flapping origin cannot re-enter the cycle until the failure knowledge expires, and it lives for a cooldown that lengthens the longer an origin keeps failing (see [Failure backoff](#failure-backoff)).

## Failure backoff

An origin whose UDP path is blocked for good would otherwise be retried at a fixed rate forever, so Fáith keeps a consecutive-failure count per origin and derives the cooldown from it.
The first failure holds the origin for `upgradeFailedTtl` (default 5 minutes) and each consecutive one doubles that, up to `upgradeFailedMaxTtl` (default 1 hour): on the defaults, 5 minutes, then 10, 20, 40, and an hour thereafter.
The cap is never less than `upgradeFailedTtl`, so setting it at or below the base gives a flat cooldown.
Every failure counts the same however it arrives, whether a foreground attempt failed, a background probe failed, or a run of cancellation strikes demoted the origin.

A confirmed HTTP/3 response clears the count, so an origin that comes good starts from the base cooldown if it later breaks again.
Otherwise the count outlives the cooldown it set by one further cooldown of the same length: an origin that fails again as soon as its cooldown lapses escalates, while one left untouched for that much longer is judged from the base again.
Counts are held per origin within the same `upgradeCacheCapacity` bound as the rest of the origin knowledge.

## Reading advertisements

The first `h3`-family service in an `Alt-Svc` header counts (draft versions like `h3-29` included); its `ma` parameter sets the advertisement lifetime, and the literal value `clear` erases knowledge as the standard requires.
An advertisement naming a different host is ignored: Fáith only upgrades to the same host.
IPv6 authorities are parsed correctly (the port split respects brackets).
A cached response is not a network exchange and neither records nor confirms anything (the HTTP cache sits outside the upgrade layer; a replayed HTTP/3-versioned cache hit must not fake a confirmation; see [CACHE](../cache/http-cache.md)).

## Hints

`http3.hints` ({host, port} pairs) declare origins the caller knows speak HTTP/3.
A hint is the user's own assertion, so it seeds the **confirmed** state directly, does not expire, and is never probed: the first request to a hinted origin speaks HTTP/3 immediately, which is what HTTP/3-only origins need.
Hints apply to `https` origins, and a hint for an origin in the failed state is refused; failures still demote hinted origins for the cooldown like any other.

## The upgrade attempt

A request to a confirmed origin is attempted over HTTP/3, with the original request preserved for fallback; on any failure the same request is retried over TCP, so callers see slower, never broken.
Only an actual HTTP/3 response confirms and keeps the origin confirmed.
A request whose body cannot be replayed (a streaming body) skips the HTTP/3 attempt for that request rather than risk an unrepeatable send.
`http3.upgradeAttemptTimeout` (default 60 seconds, 0 to disable) bounds the attempt, measured to response headers so a slow body is unaffected; expiry counts as a failure and triggers the TCP fallback.
This is the backstop for blackholed paths; real refusals arrive much faster.

## Cancellation strikes

An HTTP/3 attempt cut short from outside (abort via `signal`) is indistinguishable from a hung path, so each mid-flight cancellation counts a strike against the origin.
Reaching `upgradeCancelStrikes` (default 3) demotes the origin to failed; any successful confirmation resets the count.
Strikes only accumulate when they land within about a minute of each other; a retry loop with a longer backoff never accumulates a run, so such callers set the option to 1 for immediate demotion.
Setting 0 disables strike demotion.
One fault neither strikes nor the attempt timeout catches: a path that carries small datagrams but drops full-size ones (an MTU blackhole).
Headers arrive, so both mechanisms count success; the transfer stalls in the body, where only `maxIdleTimeout` or the request's own timeout ends it, and the origin stays confirmed.

## Advertised ports

By default, an advertisement whose port differs from the origin's is recorded but not acted on: honouring it correctly (connect to the advertised endpoint, keep the origin's authority) is not expressible in the current HTTP stack, and guessing that the origin's own port speaks HTTP/3 would be wrong.
Such origins simply don't upgrade.
`http3.upgradeFollowAdvertisedPort: true` upgrades anyway by rewriting the request's port to the advertised one (non-standards-compliant, for servers the caller controls), with three visible consequences: the request's authority carries the advertised port (servers routing on authority may misroute), `response.url` reports the port actually connected to, and `redirected` ignores port-only differences on HTTP/3 responses.
TLS still validates against the origin's hostname either way.
Origin knowledge stays keyed on the origin's port even when the request port was rewritten, and confirmations record the port proven rather than re-reading state that a concurrent failure may have cleared.
