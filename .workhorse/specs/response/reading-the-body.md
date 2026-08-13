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
Browsers return a stream there anyway; Faith follows the specification.
Accessing `body` marks the response disturbed (`bodyUsed` becomes true), even before any bytes are consumed.
A response has one body stream: `body` builds it on first access and returns that same `ReadableStream` object thereafter.
Consumption therefore advances a single position, and a handle taken after part of the body has been read continues from where the earlier one left off.
Errors surfaced through the body stream carry no `code` property (see [ERR](../errors/errors.md)).

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
It is a whole-body read alongside `bytes()` and its siblings: the first consumer wins, `bodyUsed` becomes true once the read begins, and `integrity` is verified when set (see [SRI](../fetch/integrity.md)).
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

`signal` does not reach a file write, the same as it does not reach any other body read; the per-request `timeout` and the agent's read and total timeouts bound it (see [CANCEL](../fetch/cancellation-and-timeouts.md)).
`clone()` gives a second entitlement to the body, so an original and its clone each write their own file.
`discard()` on a body already written to a file is accepted, as it is after any other read.

## clone()

`clone()` throws if the response is already disturbed.
Original and clone are separate response objects, each entitled to one full read of the body, sequentially or concurrently, receiving identical content.
Cloning does not tee the body: there is still exactly one underlying transfer, whose chunks are shared in memory between the consumers rather than duplicated into independent branches.
Trailers settle once, for original and clones alike.

## discard()

`discard()` disposes of the body so the connection can be reused, resolving when disposal is done: on HTTP/1 the body is drained; on HTTP/2 and HTTP/3 the stream is cancelled outright, since the connection is reusable regardless.
It is idempotent, and calling it on a body that has already been read is accepted rather than an error.
A discarded body cannot be read afterwards: the whole-body methods and `clone()` reject with the already-disturbed error, while `bodyUsed` stays false because disposing of a body is not reading it.
After `discard()`, the trailers promise resolves to `null` (see [TRL](trailers.md)).
An unread, undiscarded HTTP/1 response holds its connection until the response is garbage collected, at which point the body is drained and the connection returned to the pool on a best-effort basis, or closed when that is not possible.
`discard()` is the deterministic path; the collector is only the safety net.

## webResponse()

`webResponse()` returns a Web API `Response` built from the body stream, `status`, `statusText`, and `headers`: the properties a Web `Response` can be constructed with.
Faith-specific properties (`url`, `version`, `peer`, `trailers`, `redirected`) do not carry over.
It is built over the response's own body stream rather than a copy, so the conversion is available until that stream is read from or locked and refused after, as the standard does not build a `Response` over one.
Accessing `body` without reading from it does not stand in the way.
A whole-body read closes the window too, `toFile()` among them: the body is spent even though the stream was never handed out, and the conversion is refused with the already-disturbed error.
