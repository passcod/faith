# Plan — Faith-specific direct file downloads (`toFile()`)

Source of truth is the spec diff on this branch: `response/reading-the-body.md` (the `toFile()` section), `fetch/integrity.md`, `errors/errors.md`, `response/response.md`, `response/trailers.md`.

## Design notes

- `toFile()` is a whole-body read that streams body chunks straight to a file inside Rust, never crossing into JS. It reuses the existing `ensure_stream` path so `clone()` support and trailer/timing/stats bookkeeping carry through unchanged.
- **Content-Length overrun:** the header is only present in `self.headers` when the body is *not* decoded (`strip_decoded_headers` removes it on decode). So whenever `Content-Length` is visible, the bytes written equal the wire bytes — the write loop can simply count written bytes and compare against the header. No pre-decode tap needed.
- **Integrity:** stream into an `ssri::IntegrityChecker` (`Integrity::check` is literally that under the hood), finalising after the last byte. A mismatch is reported with the file already on disk.
- **Path handling:** `file://` → path conversion and `InvalidPath` rejection happen in the JS wrapper via `fileURLToPath`. Native receives a string, resolves it to absolute for the returned `path`.
- **Open ordering:** body-null check → non-committal disturbed check → open file → commit disturbed → stream. An open failure leaves the body undisturbed; an already-disturbed response creates no file.
- **Error mapping:** `AlreadyExists` → `FileExists` (directory → `FileWrite`); other open/write failures → `FileWrite`; body-read failures → `BodyStream`; overrun → `ContentLengthOverrun`; mismatch → `IntegrityMismatch`.

## Checklist

- [x] Add error kinds: `ContentLengthOverrun` (NetworkError), `FileExists`/`FileWrite` (Error), `InvalidPath`/`ResponseBodyNull` (TypeError). Update the doc-comment mapping.
- [x] Add streaming integrity helpers to `integrity.rs` (`IntegrityChecker` builder + finaliser).
- [x] Implement `to_file` + `ToFileOptions`/`ToFileResult` in `response.rs`.
- [x] Add `toFile(path, options)` to `wrapper.js` (file:// conversion, InvalidPath) and type it in `wrapper.d.ts`.
- [x] Rebuild so `index.js`/`index.d.ts` regenerate.
- [x] Update README error-codes reference and the toFile/webResponse prose the typings mirror.
- [x] Tests: `test/to-file.test.js` covering the scenarios in the test-cases file.
- [x] `webResponse()` refuses after any whole-body read (per the reading-the-body spec line added on this branch) — added a wrapper-level consumed-body flag.

## Progress reporting (`onProgress`)

Added after the initial implementation, on request.

- [x] `ToFileProgress` object and a `ProgressCallback` threadsafe-function alias in `response.rs`.
- [x] Rate-limited reporting (50ms floor) inside the write loop, plus a guaranteed final report.
- [x] `onProgress` taken from the options object in `wrapper.js` and passed as its own native
      argument — a threadsafe function cannot be a field of a `#[napi(object)]`.
- [x] `ts_args_type` on `to_file`, because napi emits a dangling type name for a
      `ThreadsafeFunction` alias otherwise.
- [x] Spec paragraph in the `toFile()` section, README section with an example, typings, tests.

`content_length` is `Option<i64>`, which napi surfaces as an absent property rather than `null`
— the docs and tests say absent to match what callers actually see.

## Note on ContentLengthOverrun

The transport (hyper) already caps a length-delimited HTTP/1 body at its `Content-Length`, so
the explicit overrun check is a backstop that a real server can't drive past. The check reads
`Content-Length` from the response headers, which is only present for a non-decoded body — where
the bytes written equal the wire bytes. A decoded body has its `Content-Length` stripped, so its
on-disk size is correctly left unconstrained. The overrun scenario in the test cases is therefore
left unticked: it isn't reliably triggerable through an HTTP server.
</content>
</invoke>
