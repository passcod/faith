---
id: BODY
---

# Reading the response body

The response body is available once, either as a stream or through a whole-body reading method. The single-consumption model follows the fetch standard's disturbed-stream semantics; what Fáith adds is explicit control over the connection cost of not reading (`discard()`) and honesty about what sharing a body means (`clone()`).

## The body stream

- [ ] `body` is a `ReadableStream` of the body contents, or `null` for responses that cannot carry a body (HEAD requests, `204 No Content`). Browsers return a stream there anyway; Fáith follows the specification.
- [ ] Accessing `body` marks the response disturbed (`bodyUsed` becomes true), even before any bytes are consumed.
- [ ] Repeated `body` accesses return streams over the same underlying source at its current position: bytes consumed through one reader are not seen by another. The body is one sequence of bytes, however many handles are taken to it.
- [ ] Errors surfaced through the body stream carry no `code` property (a technical limitation, documented as such).

## Whole-body methods

- [ ] `text()`, `json()`, `bytes()`, `arrayBuffer()`, and `blob()` read the body to completion; the first consumer wins and subsequent consumers reject with an already-disturbed error.
- [ ] `bytes()` resolves to a Node.js `Buffer` (a `Uint8Array` subclass), copied out so it cannot alias Node's shared buffer pool.
- [ ] `text()` decodes as UTF-8, replacing invalid sequences with U+FFFD rather than throwing.
- [ ] `json()` reads the full body then parses it, throwing a JSON-parse error on invalid input; peak memory is body plus parsed value.
- [ ] `blob()` sets the Blob's `type` from the `Content-Type` header, empty when absent.
- [ ] These methods verify `integrity` when set; the `body` stream path does not (see `fetch/integrity.md`).
- [ ] `formData()` exists for type compatibility and always throws.

## clone()

- [ ] `clone()` throws if the response is already disturbed.
- [ ] A clone shares the underlying body with its original: each gets its own disturbed flag, but bytes read through one are consumed from the shared stream. Trailers settle once, for original and clones alike.

## discard()

- [ ] `discard()` disposes of the body so the connection can be reused, resolving when disposal is done: on HTTP/1 the body is drained; on HTTP/2 and HTTP/3 the stream is cancelled outright, since the connection is reusable regardless.
- [ ] After `discard()`, the trailers promise resolves to `null` (see `response/trailers.md`), and `bodyUsed` remains false.
- [ ] An unread, undiscarded HTTP/1 response holds its connection until the response is garbage collected, at which point the body is drained and the connection returned to the pool on a best-effort basis, or closed when that is not possible. `discard()` is the deterministic path; the collector is only the safety net.

## webResponse()

- [ ] `webResponse()` returns a Web API `Response` built from the body stream, `status`, `statusText`, and `headers`: the properties a Web `Response` can be constructed with. Fáith-specific properties (`url`, `version`, `peer`, `trailers`, `redirected`) do not carry over.
- [ ] It always succeeds on an undisturbed response; if the body was partially read, the Web `Response` sees only the remaining bytes.
