# Upstream limitations

Some of Faith's specified behaviour is shaped by what the underlying HTTP stack can express rather than by a choice Faith made.
The specs state the behaviour plainly, because a caller depends on it either way and a spec is not the place to narrate a dependency's internals.
This register is where the causes are recorded, so that a behaviour that looks arbitrary can be traced, and so that a stack upgrade can be checked against the list to see what has become fixable.

An entry names the behaviour, the spec section that carries it, and the reason it is not simply a Faith decision.
Removing an entry means the constraint is gone, which usually means the spec section changes too.

## A TLS client certificate is presented under `credentials: "omit"`

Spec: [REQ](specs/fetch/request.md), Credentials.
The client identity is configured on the HTTP client, not per request, so a single request cannot decline to present it.

## The cookie jar ingests `Set-Cookie` under `credentials: "omit"`

Spec: [REQ](specs/fetch/request.md), Credentials, and [COOK](specs/agent/cookies.md).
Cookie storage happens inside the HTTP stack before Faith sees the response headers, so Faith can strip the header from what the caller reads but cannot stop the store from having taken it.

## Half duplex as the fetch standard describes it is not on offer

Spec: [REQ](specs/fetch/request.md), Duplex.
The standard describes a half-duplex fetch as one where the user agent sends the entire request before *processing* the response, which is more than withholding it from the caller.
Cookie ingestion and redirect following both happen inside the HTTP stack before it hands back a response, so a request whose body is still streaming has already had `Set-Cookie` stored and a redirect followed; delaying what Faith surfaces cannot come before either.
Buffering the request body first does not change this, because the stack writes the body and reads the response concurrently whatever the body was built from.
Offering the weaker reading under the standard's name would promise more than Faith delivers, which is why the option is documented as carrying no meaning rather than as a supported mode.

## QUIC connections are absent from `connections()`

Spec: [OBS](specs/agent/observability.md), connections().
A connection is registered from the local and remote addresses the HTTP stack attaches to a response, and it attaches them only for TCP-based responses; HTTP/3 responses carry no equivalent.

## `responseCount` can undercount redirects

Spec: [OBS](specs/agent/observability.md), connections().
Redirects followed inside the HTTP stack do not surface per-hop response events to the tracker.

## Errors from the `body` stream carry no `code`

Spec: [ERR](specs/errors/errors.md), The code contract.
A failure inside the stream travels back through the binding's error channel, which carries a status and a message and cannot construct a JavaScript error object to hang properties on, unlike a failure raised from a call.

## A request written into a closed connection is replayed by Faith, up to a bound

Spec: [POOL](specs/agent/connection-pool.md), Reusing a connection that has died.
The HTTP stack replays a request on a reused pooled connection only while it still owns the request message, which is the case where nothing reached the wire at all; a request written into a socket the origin had already closed is classified as a send that got no complete response, and is not replayed.
The stack also does not report whether the connection that failed came from the pool or had just been opened, which is the bound its own replay uses, so Faith bounds the number of attempts instead of the kind of connection.

## `preconnect()` sends a request to the origin

Spec: [WARM](specs/agent/warm-up.md), What preconnect sends.
The HTTP stack does not expose a way to place a connection into its pool, only to make a request that leaves one there, so a warm connection cannot be obtained without a request the origin sees.
This is also why the HTTP/3 probe is a synthetic `HEAD` rather than a bare handshake.

## `preconnect()` follows the agent's redirect policy rather than stopping at the origin

Spec: [WARM](specs/agent/warm-up.md), What preconnect sends.
The HTTP stack's redirect policy is fixed per client and has no per-request override, and its connection pool is per client too, so the raw client `preconnect` shares to land its connection in the foreground pool cannot follow a different redirect policy from foreground requests.
A warm-up therefore follows the agent's redirect policy, so a redirecting root can warm the redirect's target in addition to the origin asked for.
Moving redirect-following into Faith's own layer would let the shared client stop at the first response; it is tracked as its own card.

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

## A server timing entry is not an instance of a platform class

Spec: [RESP](specs/response/response.md), Request timing.
The platform's performance interfaces carry no `PerformanceServerTiming` class, and the call that mints a resource timing entry takes no metrics to attach, so there is nothing to make an entry an instance of and no `instanceof` for one to satisfy, unlike the breakdown that holds them.
