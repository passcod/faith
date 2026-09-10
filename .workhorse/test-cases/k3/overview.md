# Test cases: Support the QUERY method

## Method passthrough

- [x] `QUERY` reaches the origin as `QUERY`, with a body (verifies spec: REQ)
- [x] A `QUERY` body arrives at the origin intact
- [x] A `QUERY` with no body is sent as it is

## Content-Type on QUERY

- [x] A bodied `QUERY` with nothing describing the body raises `MissingContentType` (verifies spec: REQ#body)
- [x] A type on the request satisfies it
- [x] A type on the agent's default headers satisfies it
- [x] A string body's implied `text/plain` satisfies it

## Body content-type extraction

- [x] Each body kind implies the type the fetch standard extracts (verifies spec: REQ#body)
- [x] An untyped `Blob` and raw bytes imply nothing
- [x] A `FormData` implies `multipart/form-data` carrying its boundary
- [x] A type on the request outranks the agent's and the implied one
- [x] An agent's default outranks the implied one, including for `URLSearchParams`
- [x] `Blob`, `File`, and `FormData` bodies send the bytes the standard encodes

## Retry

- [x] `QUERY` counts as idempotent, so it replays on a connection that died (unit test in `retry.rs`)
- [ ] A `QUERY` replayed on a dead pooled connection reaches the origin once, end to end (verifies spec: POOL)

## Not covered

- [ ] `QUERY` over HTTP/2 and HTTP/3, rather than only HTTP/1
