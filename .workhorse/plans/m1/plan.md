# Map the fetch priority option onto the RFC 9218 Priority header

## Decisions

**Exact urgency values are left to the implementation.**
The spec constrains only which side of the scheme's default (`u=3`) each option lands on: `high` below, `low` above.
Pinning specific numbers would over-constrain a reimplementation without changing observable behaviour, since servers act on the ordering rather than the particular value.

**The incremental (`i`) parameter is not set.**
RFC 9218's `i` tells the server whether same-urgency responses should be multiplexed or served sequentially, which is a property of how the caller consumes the response body rather than of the priority hint.
The fetch standard's `priority` option has no incremental dimension to map from, and the caller cannot know at request time whether the body will be streamed or buffered.
Callers who want it can set the full `Priority` header themselves, which wins over the mapping.
Exposing it deliberately would mean a separate Fáith-specific option, not inference from `priority`.

## Notes

Precedence follows the existing pattern for headers Fáith sets: a caller-supplied or agent-default `Priority` header replaces the derived value entirely.

The agent's default headers have to be consulted explicitly.
reqwest fills a default header in only when the request doesn't carry that name, so setting the derived value on the request builder would displace an agent default rather than yield to it.
The agent already extracts `Accept-Encoding` from its default headers for the same reason, so this follows an established shape.

`priority` takes a plain string rather than a napi string enum: a string enum rejects unknown values with a type error, and an unrecognised value has to be ignored like any other option Fáith doesn't recognise.

## Steps

- [x] Map the option to a header value in `options.rs`, with unit tests for the mapping
- [x] Record on the agent whether its default headers carry `Priority`
- [x] Emit the header in `fetch.rs`, yielding to a request or agent-default `Priority`
- [x] Type the option in `wrapper.d.ts` and regenerate `index.d.ts`
- [x] Update the README entry and the compatibility stance in the overview spec
- [x] Cover the scenarios in `test/priority.test.js`
