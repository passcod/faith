# R3: read stream cancellation

## Report that prompted this

A consumer found that once response headers have arrived, nothing reaches the network:

- `signal` stops at headers, so the only thing left is cancelling the body stream.
- Cancelling the body (`reader.cancel()` or breaking out of `for await`) leaves the server's stream open, on HTTP/1.1 and on HTTP/2 over TLS, until the process exits.
- `discard()` on an endless body never settles.

## What the standard says

Fetch standard, HTTP-network fetch, where the response body stream is built:

- The stream's cancel algorithm *aborts the fetch controller* with the cancel reason.
  Cancelling the body is the same as aborting the fetch, not just letting go of a JS object.
- The in-parallel body transmission runs "but abort when fetchParams is canceled".
  When it is aborted: on HTTP/2 the user agent transmits `RST_STREAM`; otherwise it "should close connection unless it would be bad for performance to do so".
  The note gives the exception: a connection may stay open when only a few bytes remain on a reusable connection, where draining beats a new handshake.
  So on HTTP/1 the default is to close, and draining is a bounded optimisation.
- The fetch is aborted, so the stream is errored with the abort reason and the response's aborted flag is set.

`fetch()` method steps:

- The abort steps added to the request's signal are never removed.
  After the response has resolved they still abort the controller and run "abort the fetch() call", which errors the response body if it is still readable.
  So in the standard, `signal` covers the body read too.
  The CANCEL spec departs from this: "Once the response has resolved, reading the body is no longer raced against the signal."

`clone()` is a tee (Streams standard `ReadableStreamDefaultTee`):

- Cancelling one branch cancels the source only once *both* branches have cancelled.
  With clones, the network is aborted when the last consumer lets go, not the first.

`discard()` is Faith's own API with no counterpart in the standard.
The nearest thing is cancelling the body, and on HTTP/1 the standard would close the connection rather than drain it without limit.

## What Faith does, and why

The repro (HTTP/1, local endless server, one chunk read then cancel) left the server socket open in every case, including after dropping the response and forcing GC.

1. **Cancel drops a clone, not the body.**
   `Response::ensure_stream` (`crates/web-faith/src/response.rs`) wraps the body in a `SharedStream` and keeps one clone in the holder's `Body::Stream`, handing another to the JS `ReadableStream`.
   napi-rs's `cancel_callback` (napi 3.8.1, `bindgen_runtime/js_values/stream/read.rs`) drops only its own clone.
   The holder's clone keeps the reqwest body, and so the connection or HTTP/2 stream, alive as long as the response object lives.
2. **GC on HTTP/1 drains forever.**
   When the response is collected, `BodyHolder::drop` (`crates/web-faith/src/body.rs`) spawns `drain_body_inner`, which reads to the end.
   On an endless body that task never finishes, so the connection and the task leak for the life of the process.
   On HTTP/2 and HTTP/3 collection does drop the body and reset the stream, but only when GC gets round to it.
3. **`discard()` on HTTP/1 is an unbounded drain.**
   Same `drain_body_inner`, so it never resolves on an endless body.
   On HTTP/2 and HTTP/3 it replaces the holder's body with `Consumed`, which only resets the stream if no JS stream clone is still holding it.
4. **`signal` is detached once headers arrive.**
   The native fetch races the abort channel against `send` only (`crates/web-faith-napi/src/fetch.rs`), as CANCEL documents.

## Decision: follow the standard in full

- Cancelling the body stream aborts the transfer once the last consumer (response and clones) has let go: `RST_STREAM` on HTTP/2, `STOP_SENDING` on HTTP/3, the connection closed on HTTP/1 unless little enough remains that draining is cheaper.
- `signal` keeps covering the request through the body read, erroring a readable body with the abort reason.
- `discard()` and the GC safety net drain an HTTP/1 body only within a limit, then close the connection.
- `discard()` gives up only the calling response's claim on the body, like a cancel: a clone still reading keeps the transfer going, and the network is aborted when the last consumer lets go.

## Decision: claims on the body, upstream separate from the buffer

The body has two parts that can be released independently: the upstream (the network body: HTTP/1 connection, HTTP/2 or HTTP/3 stream) and the buffer (chunks already received, held for clones that have not read them yet).

- Each response and clone holds one claim on the body.
  Cancelling its stream, or calling `discard()`, gives that claim up; the garbage collector does the same for a response that is collected with its claim still held.
- Giving up a claim releases that consumer's hold on the buffer, so chunks no remaining claim still needs are freed.
- The upstream is dropped when the last claim goes, following the standard's tee rule: a clone still reading keeps the transfer going.
  On HTTP/1 a small enough remainder is drained so the connection returns to the pool; past the limit the connection is closed.
- `discard()` is therefore "give up this claim and its buffer", resolving once that is done; for the last claim it includes dropping or draining the upstream.
- `signal` belongs to the request, which all clones share, so aborting it drops the upstream at once and errors every clone's body that is still readable.
  A clone reading behind the others gets the error rather than the buffered chunks, as the standard's abort errors the stream outright.

Implementation shape:

- Wrap the upstream in a handle Faith can drop on demand, below the `SharedStream`, so dropping it does not wait for a poll and does not disturb chunks already in the shared chain.
- A consumer that reaches the end of the chain after the upstream was dropped by an abort gets an abort error, never a clean end, so a cut-off body is not mistaken for a complete one.
- Count claims explicitly rather than relying on `SharedStream` clone counts: the holder's own `Body::Stream` clone and napi-rs's clone inside the JS stream are both clones but only the latter is a consumer, and a response whose stream has not been built yet still holds a claim.
- `BodyHolder::drop` becomes "give up this claim", replacing the unbounded HTTP/1 drain.

Open: how the HTTP/1 drain limit is set (fixed internal, agent option, or agent option plus a per-call `discard()` override).

## How other clients do it

- The standard has no `Response.prototype.cancel()`; the idiom everywhere (browsers, undici, Deno, Bun) is `response.body.cancel()`, or breaking out of `for await`.
- undici's `fetch` follows the standard: the body stream's cancel aborts the controller, and its abort handler destroys the connection outright, HTTP/1 included, with no drain (`lib/web/fetch/index.js`, `onAborted`).
  `clone()` is a tee there, so cancelling one branch leaves the other reading.
- undici's lower-level `request()` API has `body.dump({ limit })`: it discards up to `limit` bytes (default 128 KiB) keeping the socket, and destroys the socket past that.
  This is the closest counterpart to Faith's `discard()`, and it is bounded.
