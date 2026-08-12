# Request priority test cases

Scenarios verifying that the `priority` option maps onto an RFC 9218 `Priority` header, and that a header set by the caller wins over the mapping.

Covered by `test/priority.test.js` against httpbin's `/headers`, which echoes the headers the request arrived with, and by unit tests on the mapping in `src/options.rs`.

## Mapping

- [x] `priority: "high"` sends a `Priority` header with an urgency below the default (verifies spec: REQ)
- [x] `priority: "low"` sends a `Priority` header with an urgency above the default (verifies spec: REQ)
- [x] `priority: "auto"` sends no `Priority` header (verifies spec: REQ)
- [x] A request with no `priority` option sends no `Priority` header (verifies spec: REQ)
- [x] An unrecognised `priority` value sends no `Priority` header and the request still succeeds (verifies spec: REQ)

## Precedence

- [x] A `Priority` header on the request is sent as written when `priority` is also set (verifies spec: REQ)
- [x] A request `Priority` header wins whatever its case, matching how header names are compared elsewhere
- [x] An agent default `Priority` header is sent as written when `priority` is also set (verifies spec: REQ)
- [x] A request `Priority` header wins over an agent default one (verifies spec: REQ)
- [x] Agent default headers that are not `Priority` leave the mapping in effect

## Request shapes

- [x] The option applies to a request with a streaming body, which reaches the native binding by a different route through the wrapper
