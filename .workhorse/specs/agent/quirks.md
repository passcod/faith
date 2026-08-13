---
id: QUIRK
---

# Quirks

The `quirks` option group holds switches that depart from standard behaviour on purpose.
Each one turns off a rule Faith otherwise upholds, in exchange for a capability the rule forbids.

Every quirk is off by default, so an agent constructed with no options is standards-compliant.
A quirk is for a caller who controls the origin, or has otherwise established that the behaviour the rule guards against does not apply to them: turning one on means requests may fail against origins that expect the standard behaviour.
Quirks affect the agent they are set on, and so every request that runs through it.

## HTTP/1.x request body streaming

`quirks.h1RequestStreaming` allows a streaming request body to be sent over an HTTP/1.x connection, which the fetch standard reserves for HTTP/2 and HTTP/3 (see [REQ](../fetch/request.md)).
With it on, a streaming body sends over whichever protocol the connection negotiates and the request is not failed for the protocol's sake.

The rule exists because an HTTP/1.x origin, or an intermediary on the path, may not accept a request whose length is unknown when the headers are sent, and a caller has no way to discover that before committing to the send.
An origin the caller controls, and has confirmed streams over HTTP/1.1, has no such uncertainty.
