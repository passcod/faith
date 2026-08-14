---
id: QUIRK
---

# Quirks

The `quirks` option group holds switches that break a rule Faith otherwise upholds, on purpose.
Each one turns off a rule stated by one of the standards Faith answers to, in exchange for a capability the rule forbids (see [FAITH](../overview.md)).

Every quirk is off by default, so an agent constructed with no options is standards-compliant.
Turning one on takes on whatever the rule guards against: requests may fail against origins or intermediaries that expect the standard behaviour, and the caller is the one who has established that they do not.
Quirks affect the agent they are set on, and so every request that runs through it.

## What belongs in the group

A switch is a quirk when it turns off a rule a standard states, whatever the reason a caller has for wanting it off.
Faith's additive extensions are the other kind of departure: they add surface the standards do not describe rather than breaking a rule any of them states, so they are always available and carry no quirk (see [FAITH](../overview.md)).
A switch that tightens behaviour, or that trades off security within what a standard permits, upholds the standard and is not a quirk either.

## HTTP/1.x request body streaming

`quirks.h1RequestStreaming` allows a streaming request body to be sent over an HTTP/1.x connection, which the fetch standard reserves for HTTP/2 and HTTP/3 (see [REQ](../fetch/request.md)).
With it on, a streaming body sends over whichever protocol the connection negotiates and the request is not failed for the protocol's sake.

The rule exists because an HTTP/1.x origin, or an intermediary on the path, may not accept a request whose length is unknown when the headers are sent, and a caller has no way to discover that before committing to the send.
An origin the caller controls, and has confirmed streams over HTTP/1.1, has no such uncertainty.

## HTTP/3 advertised ports

`quirks.h3FollowAdvertisedPort` upgrades an origin whose HTTP/3 advertisement names a port other than the origin's own, by rewriting the request's port to the advertised one (see [H3UP](../http3/upgrade.md)).
Without it such an origin does not upgrade at all.

The rule is RFC 7838's: an advertisement names a network endpoint to connect to, while the request still carries the origin's own authority.
Rewriting the port breaks that, so the advertised port travels as the request's authority, where a server routing on authority may misroute or reject it.
The same rewritten port is what `response.url` reports and what `redirected` disregards when comparing URLs.
