# Body cancellation, discard, and signal reaching the network

Scenarios for giving up a response body and for a signal aborted after the headers arrive.
The HTTP/1 cases use a local origin that serves bodies without end, so a connection left open is visible as a socket the origin never sees close.

## Giving up the body over HTTP/1

- [x] `reader.cancel()` on an endless body closes the connection while the response is still held (verifies spec: BODY)
- [x] Leaving a `for await` loop early closes the connection (verifies spec: BODY)
- [x] Cancelling while a read waits on a stalled origin ends the read and closes the connection (verifies spec: BODY)
- [x] Garbage collecting an unread response closes its connection (verifies spec: BODY)
- [x] `discard()` on an endless body settles, closes the connection, leaves `bodyUsed` false, and later reads are refused (verifies spec: BODY)
- [x] `discard()` errors this response's own stream while it is mid-read (verifies spec: BODY)

## Clones

- [x] A clone still reading keeps the transfer going after the original cancels, and the clone letting go closes it (verifies spec: BODY)
- [x] `discard()` on the original leaves a clone reading the whole body, trailers settling when the clone ends (verifies spec: BODY, TRL)
- [x] An untouched clone holds the HTTP/1 connection after the original is discarded, and discarding it too frees the connection for reuse (verifies spec: BODY, POOL)
- [x] A clone reading behind the others gets an abort ahead of buffered chunks (verifies spec: CANCEL)

## Draining HTTP/1 connections

- [x] A small remainder is drained and the connection reused by the next request (verifies spec: POOL)
- [x] `drainLimit: 0` closes the connection even for a small remainder (verifies spec: POOL)
- [x] A `Content-Length` remainder over the limit closes at once without reading it (verifies spec: POOL)
- [x] A remainder of unknown length is read up to the limit and then closed (verifies spec: POOL)
- [x] `drainTimeout` closes a remainder that stalls (verifies spec: POOL)

## HTTP/2 and HTTP/3

- [x] Cancelling an HTTP/2 body resets its stream, and the next request reuses the session (verifies spec: BODY, CANCEL)
- [x] Aborting the signal after headers on HTTP/2 resets the stream and keeps the session (verifies spec: CANCEL)
- [ ] Cancelling an HTTP/3 body stops the stream and keeps the connection (verifies spec: BODY, CANCEL)

## Signal after the headers

- [x] Aborting errors the body stream with the signal's own reason and closes the connection (verifies spec: CANCEL)
- [x] Aborting errors every clone's stream and rejects a whole-body read under way with `Aborted` (verifies spec: CANCEL)
- [x] Aborting before the unread body is read makes `json()` reject with `Aborted` (verifies spec: CANCEL)
- [x] Aborting after the body was read changes nothing (verifies spec: CANCEL)
- [x] A `toFile()` under way rejects with `Aborted` (verifies spec: BODY, CANCEL)
- [ ] Aborting after headers during an HTTP/3 attempt counts no cancellation strike (verifies spec: H3UP)

## Bookkeeping

- [x] A cancelled body resolves trailers to `null`, settles the timing, and counts as finished (verifies spec: TRL, RESP, OBS)
- [x] A body read to its end counts as started and finished once (verifies spec: OBS)

## Rust surface

- [x] Dropping a body stream gives the response's claim up (verifies spec: BODY, RSAPI)
- [ ] Dropping a `Response` with its claim held stops the transfer (verifies spec: BODY, RSAPI)
- [x] Every body stream a response hands out shares one position, and one taken after the body was read to its end sees the end (verifies spec: BODY)
