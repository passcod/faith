---
id: CANCEL
---

# Cancellation and timeouts

A request can be ended early three ways: an `AbortSignal`, a per-request `timeout`, and agent-level timeouts.
They differ in what phase they cover and what error they produce, so callers pick by failure mode.
A caller who has the response and no longer wants its body gives the body up instead, which stops the transfer once no clone still wants it (see [BODY](../response/reading-the-body.md#giving-up-the-body)).

## AbortSignal

`signal` accepts a standard `AbortSignal`; aborting it rejects the fetch with an abort error (code `Aborted`).
A signal already aborted when `fetch()` is called rejects immediately, without any network activity.
The signal covers the whole request, the body included, as the fetch standard specifies.
Aborting it after the response has resolved stops the transfer and errors the body of the response and of every clone that is still readable.
A body stream errors with the signal's abort reason, as the standard specifies, and a whole-body read or file write under way rejects with `Aborted`.
Chunks already received but not yet read are dropped with the error rather than delivered first.
A body that has been read to the end or given up is past the signal's reach, and aborting afterwards changes nothing about it.
Aborting mid-flight during an HTTP/3 attempt, before the response headers arrive, counts a cancellation strike against that origin, so a caller stuck in an abort-retry loop cannot pin an origin to a broken HTTP/3 path forever (see [H3UP](../http3/upgrade.md)).

## Per-request timeout

`timeout` (milliseconds, Faith-specific) cancels the request with a timeout error (code `Timeout`), distinguishable from a signal abort.
Unlike `signal`, it applies through the entire response receipt, including the body.

## Agent-level timeouts

`timeout.connect` bounds only the connection phase.
`timeout.read` bounds each read operation and resets after a successful read: the tool for detecting stalled connections when the response size is unknown.
`timeout.total` is a deadline for the whole request-response cycle, from connection start to body end.
All three default to unset; each produces a timeout error (code `Timeout`) when exceeded.
`dns.timeout` bounds name resolution rather than the connection or the exchange, and unlike these three it has a default (see [DNS](../agent/dns.md)).

## How the deadlines combine

A per-request `timeout` replaces `timeout.total` for that request rather than tightening it, so a request may raise or lower the agent's deadline.
`timeout.connect` and `timeout.read` apply regardless, and the first deadline to expire ends the request.
Whichever total deadline is in force runs across a redirect chain: following redirects does not restart it (see [REDIR](redirects.md)).

## What ending early does on the wire

Ending a request early stops the transfer the moment it happens: on HTTP/2 the stream is reset (`RST_STREAM`), on HTTP/3 it is stopped (`STOP_SENDING`), and in both cases the connection stays in the pool.
An HTTP/1 connection ended by a timeout, or before its response headers arrived, is closed.
An HTTP/1 connection whose signal is aborted after the response headers arrived is treated as a body given up, drained back to the pool when little enough remains and closed otherwise (see [POOL](../agent/connection-pool.md)).
