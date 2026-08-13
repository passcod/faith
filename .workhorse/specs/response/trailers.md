---
id: TRL
---

# Response trailers

`Response.trailers` is a promise resolving to the HTTP trailing headers, or `null` when there are none.
Faith implements the semantics of the fetch specification's trailers proposal (whatwg/fetch#1940).

## Resolution ordering

The promise resolves only after the body has been consumed, because trailers arrive on the wire after the body ends.
Reading the body to completion (via `text()`, `bytes()`, `json()`, `blob()`, `arrayBuffer()`, or draining the `body` stream) is what allows resolution.
Awaiting the trailers without anything consuming the body never resolves, as the proposal specifies.
Holding the pending promise while something else reads the body is supported and costs nothing.
Waiting for trailers consumes no CPU while pending: the wait parks on body completion rather than polling.
`discard()` counts as consuming the body but throws the trailers away with it: the promise then resolves to `null` rather than waiting for trailers that can no longer arrive.
A response without trailers resolves to `null` once the body ends.

## Shape

Trailers resolve as a Web API `Headers` object, the same structure as `response.headers`.
Trailers are supported on HTTP/1.1 (chunked), HTTP/2, and HTTP/3 responses.
