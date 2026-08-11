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

## Where that leaves the fix

Retry once, at the middleware layer, the way the HTTP/3 clone-fallback in
`src/alt_svc.rs` does: `try_clone()` the request up front, and on failure re-run the
clone through a fresh connection. Conditions to gate on:

- the error is the connection-died-before-any-response class, not a genuine transport
  failure, and no response bytes arrived
- `try_clone()` succeeded, so streaming bodies are excluded and stay unretryable
- one attempt only, so a permanently broken origin is not hammered

Open decision, and the reason this is not already written: whether the retry covers
non-idempotent methods. Retrying GET/HEAD/PUT/DELETE is sound on the evidence above.
Retrying POST is what the card asks for and what the half-close row argues against.
Idempotent-only matches Go; covering POST too needs a deliberate call that the
double-processing risk is worth taking.

## Steps

- [x] Add the `idleClose` capability, the `/idle/drop` and `/idle/state` routes, and
      the `aggressive idle close` dimension
- [x] Establish whether hyper and reqwest already cover this — they do not
- [ ] Decide the idempotency question above
- [ ] Implement the retry in a middleware layer
- [ ] Re-run the dimension; the eight volley assertions go green
- [ ] Regenerate the README conformance table (CI does this; it cannot be done
      faithfully on a dev machine where nginx, haproxy and quiche are absent)
