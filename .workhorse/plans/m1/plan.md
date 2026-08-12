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
