# Test cases — direct file downloads (`toFile()`)

Scenarios verifying the `toFile()` body-read path. Spec ids: BODY, SRI, ERR.

## Happy path

- [x] `toFile()` writes a response body to a fresh path and resolves to `{ path, bytesWritten }` with the absolute path and correct byte count (verifies spec: BODY)
- [x] The file on disk contains exactly the response body bytes (verifies spec: BODY)
- [x] A relative path resolves against the process working directory and the returned `path` is absolute (verifies spec: BODY)
- [x] A `file://` URL destination writes to the corresponding path (verifies spec: BODY)
- [x] A present-but-empty body writes an empty file (verifies spec: BODY)
- [x] A decoded (compressed) body is written decoded (verifies spec: BODY, ENC)

## Disturbed-stream semantics

- [x] `toFile()` sets `bodyUsed` true and a second whole-body read rejects with already-disturbed (verifies spec: BODY)
- [x] `toFile()` after another read rejects with already-disturbed and creates no file (verifies spec: BODY)
- [x] `clone()` before `toFile()` lets original and clone each write their own file (verifies spec: BODY)
- [x] `webResponse()` after `toFile()` is refused with already-disturbed (verifies spec: BODY)
- [x] `discard()` after `toFile()` is accepted (verifies spec: BODY)
- [ ] Trailers resolve after `toFile()` consumes the body (verifies spec: TRL)

## Destination handling

- [x] Default (`overwrite` unset) refuses an occupied destination with `FileExists`, leaving the existing file untouched (verifies spec: BODY, ERR)
- [x] `overwrite: true` truncates and replaces an existing file (verifies spec: BODY)
- [x] A missing parent directory fails with `FileWrite` (verifies spec: BODY, ERR)
- [x] `mode` sets the permissions of a newly created file (verifies spec: BODY)
- [x] A `file://` URL with a non-localhost host throws `InvalidPath` at the call, before the body is touched (verifies spec: BODY, ERR)

## Body-null and integrity

- [x] `toFile()` on a HEAD/204 response throws `ResponseBodyNull` and creates no file (verifies spec: BODY, ERR)
- [x] `integrity` matching passes and the file is written (verifies spec: SRI)
- [x] `integrity` mismatch throws `IntegrityMismatch` with the file left on disk (verifies spec: SRI, BODY)

## Content-Length

- [x] A body within the advertised `Content-Length` writes successfully (verifies spec: BODY)
- [ ] A server sending more than the advertised `Content-Length` fails with `ContentLengthOverrun` (verifies spec: BODY, ERR) — the transport caps a length-delimited body at its `Content-Length`, so this isn't reliably triggerable through an HTTP server

## Progress reporting

- [x] `onProgress` reports at least once, and the final report totals the whole body (verifies spec: BODY)
- [x] The final report carries the advertised `contentLength` for a body delivered as received (verifies spec: BODY)
- [x] A slow (dripped) body produces more than one report, with counts that only climb (verifies spec: BODY)
- [x] A body Faith decodes reports no `contentLength`, since the wire length is not the size on disk (verifies spec: BODY, ENC)
- [x] An empty body still reports once, having written nothing (verifies spec: BODY)
- [x] A non-function `onProgress` is refused and leaves the body untouched (verifies spec: BODY)
- [ ] A callback that throws does not fail or corrupt the write (verifies spec: BODY)

## Error codes

- [x] `ERROR_CODES` exposes `ContentLengthOverrun`, `FileExists`, `FileWrite`, `InvalidPath`, `ResponseBodyNull` (verifies spec: ERR)
</content>
