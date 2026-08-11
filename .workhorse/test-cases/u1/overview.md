# Test cases: memoise the body stream in the wrapper

Scenarios verifying that a response has one body stream, and the knock-on effects on `webResponse()`.
Automated cases run against go-httpbin (`HTTPBIN_URL`); `/stream-bytes/{n}?chunk_size=` gives a multi-chunk body to read partially.

## The body stream

- [x] `body` returns the same `ReadableStream` object on every access (verifies spec: BODY#the-body-stream)
- [x] `body` is `null` on every access for a response that cannot carry one, HEAD included (verifies spec: BODY#the-body-stream)
- [x] Reading one chunk, releasing the reader, then taking `body` again continues from that point: the two handles together cover the body exactly once, with no replay (verifies spec: BODY#the-body-stream)
- [x] Accessing `body` marks the response disturbed before any bytes are read (verifies spec: BODY#the-body-stream)
- [x] A body read to completion through the stream leaves further reads on the same handle done, rather than restarting (verifies spec: BODY#the-body-stream)

## Interaction with the other read paths

- [x] `clone()` taken before the original's body is accessed still reads the full body (verifies spec: BODY#clone)
- [x] Original and clone hold separate streams with independent cursors, each seeing the whole body (verifies spec: BODY#clone)
- [x] A whole-body method after `body` has been accessed rejects with the already-disturbed error (verifies spec: BODY#whole-body-methods)
- [ ] `body` after `discard()` is refused rather than handing out a stream (verifies spec: BODY#discard)

## webResponse()

- [x] `webResponse()` succeeds after `body` has been accessed but not read from (verifies spec: BODY#webresponse)
- [x] `webResponse()` is refused with a `TypeError` once the body stream has been read from (verifies spec: BODY#webresponse)
- [x] `webResponse()` is refused while a reader is still held on the body stream (verifies spec: BODY#webresponse)
- [x] The Web `Response` reads the same stream as the Fáith response, not a copy (verifies spec: BODY#webresponse)

## Connections

- [ ] A body left unread after `body` has been memoised still returns its connection to the pool once the response is collected (verifies spec: BODY#discard)
