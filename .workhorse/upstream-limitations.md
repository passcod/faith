# Upstream limitations

Some of Fáith's specified behaviour is shaped by what the underlying HTTP stack can express rather than by a choice Fáith made.
The specs state the behaviour plainly, because a caller depends on it either way and a spec is not the place to narrate a dependency's internals.
This register is where the causes are recorded, so that a behaviour that looks arbitrary can be traced, and so that a stack upgrade can be checked against the list to see what has become fixable.

An entry names the behaviour, the spec section that carries it, and the reason it is not simply a Fáith decision.
Removing an entry means the constraint is gone, which usually means the spec section changes too.

## A TLS client certificate is presented under `credentials: "omit"`

Spec: [REQ](specs/fetch/request.md), Credentials.
The client identity is configured on the HTTP client, not per request, so a single request cannot decline to present it.

## The cookie jar ingests `Set-Cookie` under `credentials: "omit"`

Spec: [REQ](specs/fetch/request.md), Credentials, and [COOK](specs/agent/cookies.md).
Cookie storage happens inside the HTTP stack before Fáith sees the response headers, so Fáith can strip the header from what the caller reads but cannot stop the store from having taken it.

## QUIC connections are absent from `connections()`

Spec: [OBS](specs/agent/observability.md), connections().
A connection is registered from the local and remote addresses the HTTP stack attaches to a response, and it attaches them only for TCP-based responses; HTTP/3 responses carry no equivalent.

## `responseCount` can undercount redirects

Spec: [OBS](specs/agent/observability.md), connections().
Redirects followed inside the HTTP stack do not surface per-hop response events to the tracker.

## Errors from the `body` stream carry no `code`

Spec: [ERR](specs/errors/errors.md), The code contract.
A failure inside the stream travels back through the binding's error channel, which carries a status and a message and cannot construct a JavaScript error object to hang properties on, unlike a failure raised from a call.

## An advertised HTTP/3 port cannot be honoured while keeping the origin's authority

Spec: [H3UP](specs/http3/upgrade.md), Advertised ports.
Connecting to one endpoint while sending another authority is not expressible in the HTTP stack, which is why the default is to record the advertisement without acting on it.
