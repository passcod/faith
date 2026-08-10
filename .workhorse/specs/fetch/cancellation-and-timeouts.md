---
id: CANCEL
---

# Cancellation and timeouts

A request can be ended early three ways: an `AbortSignal`, a per-request `timeout`, and agent-level timeouts.
They differ in what phase they cover and what error they produce, and those differences are the point: callers pick by failure mode.

## AbortSignal

`signal` accepts a standard `AbortSignal`; aborting it rejects the fetch with an abort error (code `Aborted`).
A signal already aborted when `fetch()` is called rejects immediately, without any network activity.
The signal covers the request up to response headers.
Once the response has resolved, reading the body is no longer raced against the signal.
Aborting mid-flight during an HTTP/3 attempt counts a cancellation strike against that origin, so a caller stuck in an abort-retry loop cannot pin an origin to a broken HTTP/3 path forever (see [H3UP](../http3/upgrade.md)).

## Per-request timeout

`timeout` (milliseconds, Fáith-specific) cancels the request with a timeout error (code `Timeout`), distinguishable from a signal abort.
Unlike `signal`, it applies through the entire response receipt, including the body.

## Agent-level timeouts

`timeout.connect` bounds only the connection phase.
`timeout.read` bounds each read operation and resets after a successful read: the tool for detecting stalled connections when the response size is unknown.
`timeout.total` is a deadline for the whole request-response cycle, from connection start to body end.
All three default to unset; each produces a timeout error (code `Timeout`) when exceeded.
