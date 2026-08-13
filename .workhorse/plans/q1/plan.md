# Support full duplex mode

Tracked as passcod/faith#3, which records the request with no design.
This plan holds the investigation that establishes the scope.

## Finding: Faith is already full duplex

Measured on this branch, against local servers that reply before reading the request body.
Both plain HTTP/1.1 and HTTP/2 (TLS, ALPN) behave the same way:

- Response headers resolve while the request body stream is still open (h1: +20ms against a body held open to +1511ms; h2: +46ms against +1502ms).
- Response body chunks can be read while the request body is still open (h2 first chunk at +58ms).
- A genuine interactive ping-pong works: three round-trips where each request chunk is written only after reading the server's reply to the previous one, over plain HTTP/1.1. Under half duplex this deadlocks.

Node's built-in fetch (undici) was run through the identical probes as a control and behaves identically.

So the wire-level capability is not the work.
It falls out of the reqwest/hyper stack, which drives the body write and the response read concurrently, and out of `wrapper.js` never awaiting the body pump before returning the response (the pump at wrapper.js:633 is deliberately not awaited).

## What the work actually is

The gap is between behaviour and contract. Everything below currently asserts the opposite of what the code does:

- `.workhorse/specs/fetch/request.md`, Body: "Faith operates in half duplex: the whole request is sent before the response is processed."
- `README.md:288`, `wrapper.d.ts:158`, `index.d.ts:1102`, `src/options.rs:122`: "Faith will send the entire request before processing the response."
- `README.md:159`: a commented-out section describing full duplex as not yet implemented.
- `DuplexOption` (src/options.rs:127) has a single `Half` variant, and the option is inert: nothing reads it beyond the presence check in `wrapper.js:587`.

No test covers duplex sequencing. `test/duplex.test.js` covers the `duplex` option's validation and streaming uploads, but nothing asserts when the response resolves relative to the body.

## Ecosystem position (checked 2026-08-14)

- The fetch standard still has `enum RequestDuplex { "half" }`. `"full"` is reserved for future use, pointing at whatwg/fetch#1254 for defining it.
- whatwg/fetch#1254 is open, labelled "needs implementer interest", opened 2021-06-16, last activity 2025-07-11. No browser implements full duplex.
- Node (undici) is always full duplex and the `duplex` option does not affect behaviour; maintainers confirmed this and documented it.
- Deno defaults to full duplex for streaming fetches, deviating from the spec deliberately.
- Bun does not full duplex and hangs when attempted (oven-sh/bun#7206).

Faith is a server-side library, so undici and Deno are the relevant precedent, and Faith already matches them.
The README's framing that browsers lack full duplex is still correct; what changed is that Faith turns out to be on the implemented side of that line already.

## What the standard actually says

Worth pinning down, because the answer is "less than the prose suggests".

`duplex` has exactly one normative effect in the whole fetch standard: in the `Request` constructor, "If initBody is non-null and init["duplex"] does not exist, then throw a TypeError."
No algorithm step anywhere reads its *value*. The `duplex` getter is specified to return `"half"` unconditionally, so it reports the enum rather than anything the fetch did.

The half-duplex description ("the user agent sends the entire request before processing the response") lives only in the prose describing the dictionary member.
It is a gloss, not an algorithm, and it is stronger than a delivery gate: *processing* the response covers parsing headers and reading the body off the wire, not just handing it to the caller.

The algorithm in HTTP-network fetch imposes no ordering between the two.
Making the request waits until the response headers are transmitted; transmitting the request body is a separate sub-procedure whose chunks are transmitted "in parallel".
Nothing gates the header wait on the body finishing.
That absence is the reason whatwg/fetch#1254 is about *defining* `"full"` rather than about permitting it.

So there is no standard-specified behaviour for Faith to conform to here, in either direction. The choice is Faith's to make and to document.

### A separate divergence this turned up

The standard requires HTTP/2 or later for a streaming request body: "If connection is an HTTP/1.x connection, request's body is non-null, and request's body's source is null, then return a network error", where a null source means the body came from a `ReadableStream`.
Faith streams request bodies over HTTP/1.1 without complaint, and full duplex works there (measured above).
This is a deliberate-looking capability rather than a bug, but it is undocumented, and it bears on this card: whichever duplex modes Faith offers, it offers them on a protocol the standard rules out for streaming uploads at all.

## The standard's reading is not reachable on this stack

Spiked and measured, not reasoned about.

The gate delays handing the response to the caller. It does not stop the stack processing it, and the processing is observable from outside while the request body is still open:

- **Cookies.** With a jar enabled and an origin that answers with `Set-Cookie` without reading the body, `agent.getCookie()` returned `probe=1` at +752ms, with the body held open to +1500ms.
- **Redirects.** An origin that answers `302` before reading the body had its redirect followed mid-flight: the server saw `GET /landed` at +755ms while the original request's body was still streaming.

Both happen inside reqwest before `send()` resolves, so no gate placed after `send()` can precede them.
This is the same root cause as two entries already in [the upstream limitations register](../../upstream-limitations.md): cookie storage happening inside the stack before Faith sees the headers, and redirect policy being fixed per client.

Buffering the request body first does not rescue it either. A 32MiB buffered body against an origin that never read it still had its response processed at +18ms, because the stack writes the body and reads the response concurrently regardless of where the body came from.

Reaching the standard's reading would mean moving redirect following and cookie ingestion into Faith's own layer, and even then response headers are parsed by the stack the moment they arrive.
That is a much larger change than this card, and it is bounded by what the stack exposes rather than by effort.

So the answer to "can we support both" does not change, but what `half` can promise does: Faith can offer the gate, and cannot offer "no response processing until the request is sent".

## Feasibility: both modes are supportable

Established by a working spike on this branch, not by reasoning.
The spike implements half duplex as a gate on when the response is surfaced: the stack stays duplex underneath, and Faith withholds the response until the request body has gone out.
That is the cheap reading of half duplex, and it is weaker than the standard's prose gloss, which would have Faith not process the response at all until the request was sent.
Whether the weaker reading is the one to ship is an open decision below.

Mechanism: a drop guard rides along with the request body stream (`SentGuard` in src/stream_body.rs) and fires a oneshot when the stack finishes with the body. `fetch.rs` awaits that signal after `send()` resolves, before surfacing the response, unless the caller asked for `full`.

Measured against an origin that answers without reading the request body:

| | HTTP/1.1 | HTTP/2 |
|---|---|---|
| `duplex: "full"` | response at +19ms | +53ms |
| `duplex: "half"` | +1200ms, matching the body close | +1203ms |

The whole JS suite (1795 assertions) and the Rust tests (125) pass with the gate in place.

### What the spike does not yet cover

- **Buffered bodies are not gated.** A 32MiB `Buffer` body with `duplex: "half"` surfaced its response at +18ms against an origin that never read it, so the body cannot have been sent. Only streaming bodies carry the completion signal. Closing this means wrapping the buffered body in a stream and setting `Content-Length` explicitly, since `wrap_stream` otherwise drops to chunked encoding.
- **The gate is not covered by `timeout`.** Against a raw origin that answers and then never reads, a half-duplex request stalled for 30.5s before failing, with a 4s `timeout` set. The per-request timeout is handed to reqwest and stops applying once `send()` resolves, so the wait for the body needs to sit under the timeout and the abort signal itself. The spike races the gate against the signal but not the timeout.
- **A stalled origin cannot be distinguished from a slow one.** The guard fires when the stack finishes with the body, which includes the case where the exchange ended with the body undelivered. An origin that replied and ignored the body resolved at +78ms having sent 4MiB of 64MiB. So the guarantee half duplex can offer is "the stack is done with the request body", not "every byte reached the origin".

## Open decisions

1. Which mode is the default?
   Today's behaviour is full duplex for streaming bodies, so defaulting to half is a behaviour change for anyone already relying on it, and defaulting to full leaves `duplex: "half"` naming a mode the caller does not get unless they know to ask. The spike currently defaults to half, which is what makes the existing suite exercise the gate.

2. Is `duplex: "full"` the way a caller asks for it?
   It reads naturally and matches the value the standard reserves, at the cost of accepting a value no browser accepts. The alternative is a Faith-specific option that does not collide with the standard's enum.

3. What does Faith's `half` promise, given the standard's reading is out of reach?
   Deliverable: the response is not surfaced until the stack is done with the request body.
   Not deliverable: that every byte reached the origin, and that nothing about the response was processed first (cookies stored and redirects followed both precede the gate, measured above).
   The wording needs to describe the gate honestly rather than borrow the standard's phrasing, which Faith would not be meeting.

4. Does the guarantee need locking in with tests?
   Full duplex currently holds because of how the stack composes rather than because Faith asks for it, so a stack upgrade could take it away silently. Nothing in the suite covers duplex sequencing today.
