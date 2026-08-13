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

## Open decisions

1. Does `duplex: "full"` become an accepted value?
   Accepting it lets callers state intent and makes the capability discoverable, at the cost of a non-standard enum value.
   Leaving the enum at `half` alone matches undici, where the option is inert and the docs carry the explanation.

2. Does `duplex: "half"` keep meaning nothing?
   Honouring it literally, by withholding the response until the body is sent, would be spec-conformant and would make the option meaningful, but it changes today's behaviour, costs the capability by default, and matches no other server-side runtime.

3. How much of this is guaranteed rather than incidental?
   Full duplex currently holds because of how the stack composes, not because Faith asks for it. Committing to it in a spec means tests that would catch a stack upgrade taking it away.
