---
id: BODY
---

# Reading the response body

The response body is read from the network once and delivered either as a stream, through a whole-body reading method, or straight to a file on disk.
The whole-body methods follow the fetch standard's disturbed-stream semantics: the first consumer wins and later ones are refused.
`clone()` is the sanctioned way to obtain a second consumer, and `discard()` gives explicit control over the connection cost of not reading.
A body Faith decodes is delivered decoded whichever path reads it (see [ENC](../fetch/content-encoding.md)).

## The body stream

`body` is a `ReadableStream` of the body contents, or `null` for responses that cannot carry a body (HEAD requests, `204 No Content`).
Browsers return a stream there anyway; Faith follows the standard.
Accessing `body` marks the response disturbed (the body-used flag becomes true), even before any bytes are consumed.
A response has one body stream: `body` builds it on first access and returns that same `ReadableStream` object thereafter.
Consumption therefore advances a single position, and a handle taken after part of the body has been read continues from where the earlier one left off.
Errors surfaced through the body stream carry no `code` property (see [ERR](../errors/errors.md)).
Cancelling the stream, whether by `cancel()` on it or its reader or by leaving a `for await` loop early, gives up the response's claim on the body (see [Giving up the body](#giving-up-the-body)).

## Whole-body methods

`text()`, `json()`, `bytes()`, `arrayBuffer()`, and `blob()` read the body to completion; the first consumer wins and subsequent consumers reject with an already-disturbed error.
`bytes()` resolves to a Node.js `Buffer` (a `Uint8Array` subclass), copied out so it cannot alias Node's shared buffer pool.
`text()` decodes as UTF-8, replacing invalid sequences with U+FFFD rather than throwing.
`json()` reads the full body then parses it, throwing a JSON-parse error on invalid input; peak memory is body plus parsed value.
`blob()` sets the Blob's `type` from the `Content-Type` header, empty when absent.
These methods verify `integrity` when set; the `body` stream path does not (see [SRI](../fetch/integrity.md)).
`formData()` exists for type compatibility and always throws.

## toFile()

`toFile(path, options)` writes the body to a file on disk, the bytes travelling from the network to the filesystem inside Faith without crossing into JavaScript.
It is a whole-body read alongside `bytes()` and its siblings: the first consumer wins, the body-used flag becomes true once the read begins, and `integrity` is verified when set (see [SRI](../fetch/integrity.md)).
A caller wanting a file on disk therefore has no reason to route the body through a `ReadableStream` and Node's filesystem APIs.
It resolves to `{ path, bytesWritten }`, where `path` is the absolute filesystem path written to and `bytesWritten` counts the bytes that landed there.
The write runs on Faith's own async runtime rather than the libuv worker pool (see [RESP](response.md)).

The destination is named by a string path or a `file://` URL, and a relative path resolves against the process's working directory.
Paths are text, resolved to a string before the write begins.
A `file://` URL is converted to a path in JavaScript, by the platform's own conversion, before the request reaches Faith's native layer.
A URL that does not name a local path therefore throws `InvalidPath` at the call, before the body is touched: one carrying a host other than `localhost`, or one whose path encodes a separator.
A destination that is well-formed but cannot be written to, an existing directory among them, is not knowable without asking the filesystem and surfaces as `FileWrite` when the open fails.
`overwrite` governs an occupied destination and defaults to false, so the safe case is the one a caller gets without asking for it: the write fails with `FileExists` and the file already there is left as it was.
`overwrite: true` truncates it instead.
`mode` sets the permissions a newly created file is given, defaulting to what Node's own filesystem writes use.
The parent directory must already exist.

The destination is opened before any of the body is read, so a failure to open it leaves the body unread and undisturbed and the caller free to retry to another path.
A response that cannot carry a body has nothing to write and throws `ResponseBodyNull` without creating a file; a response whose body is present but empty writes an empty one.
Every other failure to open or write is a `FileWrite` error carrying the operating system's own detail: permission refused, no such directory, no space left, a write failing part way through.

A failure part way through leaves the bytes written so far sitting at the destination and throws.
Faith does not tidy up after itself here, so a caller who needs the destination path to hold either a whole body or nothing writes to a temporary path and renames on success.
An integrity mismatch is one of these failures: the digest is only known once the last byte has been written, so the file that fails verification is on disk when the error arrives.

The bytes written are the bytes any other read path would deliver, so a body Faith decodes is written decoded (see [ENC](../fetch/content-encoding.md)).
Where the response advertised a `Content-Length`, Faith holds the server to it as the body arrives, measuring the encoded bytes off the wire before decoding and failing with `ContentLengthOverrun` once they exceed the advertisement.
So a caller reads the advertised length from the response headers, decides whether it is willing to spend that much disk, and knows a server cannot then send more than it promised.
The check is `toFile()`'s alone: the other read paths hand the body back as a value the caller can size and drop, while a file write spends a resource on the caller's behalf that outlives the process.
The number constrained is the wire length rather than the size on disk, so a caller wanting the two to be the same requests `Accept-Encoding: identity`, which also stops Faith decoding (see [ENC](../fetch/content-encoding.md)).

`onProgress` is a function the write reports to as the bytes land, receiving the count written so far and the total the response advertised, or nothing for a total that cannot be known ahead of time: a chunked response, or one Faith decodes, where the size on disk is not the length on the wire.
Reports are rate limited rather than one per chunk, because a body large enough to be worth watching is one whose chunks are numerous enough that reporting each would spend more crossing into JavaScript than the write saves by staying out of it.
The last report is always delivered, so a caller's final view of a completed write is the whole body rather than wherever the rate limit last landed, and a write with nothing to report still reports once.
Progress is observational: it does not pace, pause, or fail the write, and a write with no callback behaves the same in every other respect.

`signal` reaches a file write as it reaches any other body read: aborting it fails the write with `Aborted`, leaving what was written so far on disk like any other failure part way through.
The per-request `timeout` and the agent's read and total timeouts bound the write too (see [CANCEL](../fetch/cancellation-and-timeouts.md)).
`clone()` gives a second entitlement to the body, so an original and its clone each write their own file.
`discard()` on a body already written to a file is accepted, as it is after any other read.

## clone()

`clone()` throws if the response is already disturbed.
Original and clone are separate response objects, each entitled to one full read of the body, sequentially or concurrently, receiving identical content.
Cloning does not tee the body: there is still exactly one underlying transfer, whose chunks are shared in memory between the consumers rather than duplicated into independent branches.
Trailers settle once, for original and clones alike.
A chunk stays in memory until every response still holding a claim has read past it, so a clone that is never read keeps everything the others have read until it gives its claim up.

## Giving up the body

The original response and each of its clones hold one claim on the body, and the transfer continues for as long as any claim does.
A claim is spent by reading the body to the end, and given up early by cancelling the body stream, by `discard()`, or by the response being garbage collected.
On the Rust surface, dropping a body stream or a response gives its claim up the same way (see [RSAPI](../rust/client-api.md)).
Giving up a claim releases that response's hold on the chunks already received, so the ones no remaining claim needs are freed.
A clone still reading when another gives up carries on unaffected, as a branch of the standard's tee does.

When the last claim is given up before the body has ended, Faith stops the transfer, as the fetch standard does when a body stream is cancelled.
On HTTP/2 the stream is reset (`RST_STREAM`) and on HTTP/3 the response side of the stream is stopped (`STOP_SENDING`), leaving the multiplexed connection in the pool.
On HTTP/1 the connection can only be reused once the body has been read to its end, so Faith reads out a small remainder and returns the connection to the pool, and closes the connection when the remainder is larger.
The agent's drain limit and drain timeout decide which, and a drain that reaches either closes the connection (see [POOL](../agent/connection-pool.md)).
Stopping the transfer happens as soon as the last claim goes, without waiting for anything to read from the body again, so an endless body costs nothing once no one wants it.
The trailers promise then resolves to `null` and the request's timing settles (see [TRL](trailers.md) and [RESP](response.md)).

## discard()

`discard()` gives up the response's claim on the body and resolves once it has been given up (see [Giving up the body](#giving-up-the-body)).
When it is the last claim, that includes stopping the transfer, so the promise settles after the HTTP/2 or HTTP/3 stream is reset or the HTTP/1 connection has been drained back to the pool or closed.
The drain limit and drain timeout bound that, so `discard()` settles whatever the server does, and a drain that ends in closing the connection is not an error.
A clone still reading keeps the transfer going, and `discard()` on the original does not interrupt it.
It is idempotent, and calling it on a body that has already been read is accepted rather than an error.
A discarded body cannot be read afterwards: the whole-body methods and `clone()` reject with the already-disturbed error, a body stream of this response still being read errors, and the body-used flag stays false because disposing of a body is not reading it.
After `discard()`, the trailers promise resolves to `null` unless a clone goes on to read the body to the end (see [TRL](trailers.md)).
An unread, undiscarded response holds its claim until it is garbage collected, which on HTTP/1 holds its connection that long.
`discard()` is the deterministic path; the collector is only the safety net.

## webResponse()

`webResponse()` returns a Web API `Response` built from the body stream, `status`, `statusText`, and `headers`: the properties a Web `Response` can be constructed with.
Faith-specific properties (`url`, `version`, `peer`, `trailers`, `redirected`) do not carry over.
It is built over the response's own body stream rather than a copy, so the conversion is available until that stream is read from or locked and refused after, as the standard does not build a `Response` over one.
Accessing `body` without reading from it does not stand in the way.
A whole-body read closes the window too, `toFile()` among them: the body is spent even though the stream was never handed out, and the conversion is refused with the already-disturbed error.
