# Retry transparently when a reused pooled connection dies

## The conformance dimension came first, and it found a gap

`aggressive idle close` (`test/conformance/dimensions/idle-close.js`) runs on the
node-h1 row, against a `/idle/drop` route that answers normally and then closes the
connection underneath without any in-band signal. It fills the pool with a volley of
concurrent requests, has the origin abandon all of them, and immediately asks for
them back.

The gap is real. Of 64 requests, 20 to 25 fail on a run, with:

```
hyper_util::client::legacy::Error(SendRequest, hyper::Error(IncompleteMessage))
```

surfacing to the caller as a `Network` error on a request the origin never received.
The same requests against a never-pooled agent (fresh `Agent` per request) fail zero
times out of sixteen, so this is connection reuse, not truncation or a response-
handling defect.

## hyper's built-in retry does not cover this

hyper-util 0.1.19 does retry a reused pooled connection, and `retry_canceled_requests`
defaults to true (faith never sets it). But the retry is gated on hyper still owning
the request message — `err.take_message()` returning `Some`, classified `Canceled` and
`Retryable`. That is the never-written-to-the-wire case only.

Here the bytes were written, into a socket the origin had already closed, and the
error comes back as `SendRequest`/`IncompleteMessage`, which hyper classifies `Nope`.
So the retry never fires. This is the same case Go's `net/http` calls
`errServerClosedIdle`, and Go does retry it — but only for replayable requests.

## The card's premise is half right, and the half that is wrong matters

The card justifies retrying non-idempotent requests with "nothing was processed". That
does not follow from "no response bytes arrived", and it is measurable which way it
falls:

| origin closes with | client failures (of 32) | processed but never answered |
| --- | --- | --- |
| `end()` — half close, read side stays open | 11 | **11** |
| `end()` then `destroy()` — both directions | 7 | 0 |

Under a full close, the failed requests genuinely never reached the origin, and the
card's reasoning holds. Under a half close, the origin parsed and handled every one of
the requests it could no longer answer — retrying those would double-process them. The
client sees `IncompleteMessage` either way and cannot tell which happened.

The conformance route does the full close, because that is what a real origin closing
an idle connection does, and a half-closed route measures a scenario nothing in the
wild produces. But the half-close result is why the retry cannot be justified on "no
response arrived" alone.

## The decision: idempotent methods only

`POST` and `PATCH` are not replayed. The half-close row above is the argument: a
connection that died carries no evidence of whether the origin processed the request
before it went, so a request whose repetition would count twice surfaces the failure
instead. This matches Go, and it means callers talking to an aggressively-closing
origin still see occasional errors on non-idempotent requests. That is the accepted
cost, not an oversight.

## The retry needed a bound, not a single attempt

Implemented as `DeadConnectionRetry` in `src/retry.rs`, registered last in
`Agent::new` so it sits innermost — inside the Alt-Svc layer, so a failed HTTP/3
attempt stays the fallback's business, and inside the HTTP cache, so a replay
re-sends rather than redoing the lookup.

One replay was not enough, and the reason is worth keeping. An origin that closes
idle connections closes all of them, so the pool comes back holding several that are
already gone, and the replay draws from that same pool. Measured against the
dimension, with the debug build:

| replays | GET failures |
| --- | --- |
| 1 | 58 of 135 assertions |
| 3 | 6 of 135 |
| 4 | 0 of 360 |
| 5 | 0 of 360 |

Settled on five. Each failed attempt has at least evicted one dead connection, so
what is needed is bounded by how many the pool was holding rather than by anything
about the origin. The bound is a safety valve for an origin that ends every
connection this way — indistinguishable from a full pool, since nothing in the error
says whether the connection was reused — with the request's own timeout as the outer
backstop.

## Steps

- [x] Add the `idleClose` capability, the `/idle/drop` and `/idle/state` routes, and
      the `aggressive idle close` dimension
- [x] Establish whether hyper and reqwest already cover this — they do not
- [x] Decide the idempotency question: idempotent only
- [x] Implement the replay middleware and wire it in innermost
- [x] Re-run the dimension: green over three consecutive full runs
- [x] Spec the behaviour in POOL, and record the cause in the upstream-limitations
      register
- [ ] Cover the unticked cases in `.workhorse/test-cases/f1/overview.md` — the replay
      conditions (streaming body, refused connection, timeout, 5xx, abort) are
      asserted by construction in the code but not yet by a test
- [x] Regenerate the README conformance table. CI renders it and fails on
      `git diff --exit-code`, so it has to be in the commit rather than left to CI.
      A dev machine without nginx, haproxy and quiche cannot render it from a real
      run, but the realised matrix can be reconstructed from `expected-matrix.json`
      once CI reports every cell passing, and rendering that gives the same table
