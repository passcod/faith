# Retry transparently when a reused pooled connection dies

Coverage for replaying a request whose pooled connection the origin had already closed.
The scenario is a race, so the conformance cases below make it likely and repeat it rather than forcing it; a run that never loses the race verifies nothing, which is why the origin-side count is asserted alongside.

## Conformance

Covered by the `aggressive idle close` dimension on the node-h1 row.

- [x] A GET on a pooled connection the origin abandoned reaches the caller as a success, not a connection error (verifies spec: POOL)
- [x] A POST on such a connection never comes back with a wrong status or a wrong body (verifies spec: POOL)
- [x] The origin really did close the connection under every request it answered, so a passing run means the scenario happened
- [x] The row is HTTP/1, so the pool holds one connection per in-flight request

## Replay conditions

- [x] The idempotent methods are the replayable ones, and `POST`, `PATCH` and `CONNECT` are not (verifies spec: POOL)
- [ ] A `PUT` or `DELETE` on a dead pooled connection is replayed and succeeds, the same as a GET (verifies spec: POOL)
- [ ] A request with a `ReadableStream` body is not replayed, and the failure reaches the caller (verifies spec: POOL)
- [ ] A refused connection is returned to the caller without being replayed (verifies spec: POOL)
- [ ] A timeout is returned to the caller without being replayed, and the replays do not extend the deadline (verifies spec: POOL)
- [ ] A 5xx response is returned as it is, never replayed (verifies spec: POOL)
- [ ] An origin that ends every connection this way, fresh ones included, surfaces the failure after the bounded attempts rather than looping (verifies spec: POOL)

## Interaction with the other layers

- [ ] A failed HTTP/3 attempt still falls back to TCP, and the replay layer does not re-run the upgrade decision or record a second failure against the origin (verifies spec: H3UP)
- [ ] A cache hit is served without any replay, and a replayed request does not redo the cache lookup (verifies spec: POOL)
- [ ] An aborted request is not replayed after the signal fires

## Regression

- [x] The existing suite passes unchanged, including the polite `connection reuse` dimension where the origin sends `Connection: close`
