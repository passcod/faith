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

## A request written into a closed connection is replayed by Fáith, up to a bound

Spec: [POOL](specs/agent/connection-pool.md), Reusing a connection that has died.
The HTTP stack replays a request on a reused pooled connection only while it still owns the request message, which is the case where nothing reached the wire at all; a request written into a socket the origin had already closed is classified as a send that got no complete response, and is not replayed.
The stack also does not report whether the connection that failed came from the pool or had just been opened, which is the bound its own replay uses, so Fáith bounds the number of attempts instead of the kind of connection.

## `preconnect()` sends a request to the origin

Spec: [WARM](specs/agent/warm-up.md), What preconnect sends.
The HTTP stack does not expose a way to place a connection into its pool, only to make a request that leaves one there, so a warm connection cannot be obtained without a request the origin sees.
This is also why the HTTP/3 probe is a synthetic `HEAD` rather than a bare handshake.

## An advertised HTTP/3 port cannot be honoured while keeping the origin's authority

Spec: [H3UP](specs/http3/upgrade.md), Advertised ports.
Connecting to one endpoint while sending another authority is not expressible in the HTTP stack, which is why the default is to record the advertisement without acting on it.

## Connection setup and request writing are absent from the timing breakdown

Spec: [RESP](specs/response/response.md), Request timing.
The HTTP stack reports neither the boundaries of connection setup (DNS resolution, TCP connect, TLS handshake) nor the moments a request's head and body are written, so the phases covering them have no measurement to carry.
The stack also does not surface interim (1xx) responses, so a `103 Early Hints` cannot be timed, nor a wire-level byte count, so the transfer and body sizes have nothing to count.

## Reuse is unknown for a request that travelled over QUIC

Spec: [RESP](specs/response/response.md), Request timing.
Reuse is read from the agent's connection tracker, which registers a connection from the local and remote addresses the HTTP stack attaches to a response, and attaches them only for TCP-based responses; this is the same constraint that keeps QUIC connections out of `connections()`.
