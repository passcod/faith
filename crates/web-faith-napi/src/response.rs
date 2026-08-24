use std::{
	fmt::Debug,
	result::Result,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use futures::TryStreamExt;
use napi::{
	bindgen_prelude::*,
	threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;

use web_faith::response::{FileDestination, FileProgress, FileWritten, Response, Trailers};

use crate::{
	async_task::{Value, faith_promise},
	error::{FaithError, FaithErrorExt, FaithErrorKind},
	timing::TimingBreakdown,
};

/// Options for `toFile()`.
#[napi(object)]
#[derive(Debug, Default)]
pub struct ToFileOptions {
	/// Whether to truncate and replace an occupied destination. Defaults to false, which
	/// refuses an occupied destination with a `FileExists` error and leaves it untouched.
	pub overwrite: Option<bool>,
	/// The permissions a newly created file is given, defaulting to what Node's own
	/// filesystem writes use. Ignored on platforms without Unix file modes.
	pub mode: Option<u32>,
}

/// What `toFile()` resolves to.
#[napi(object)]
#[derive(Debug)]
pub struct ToFileResult {
	/// The absolute filesystem path written to.
	pub path: String,
	/// The number of bytes that landed at the destination.
	pub bytes_written: i64,
}

/// A progress report from a `toFile()` write in flight.
#[napi(object)]
#[derive(Debug)]
pub struct ToFileProgress {
	/// The number of bytes written to the file so far.
	pub bytes_written: i64,
	/// What the response advertised in `Content-Length`, when it sent one and Faith is
	/// not decoding the body. Absent when the total is not known ahead of time, which is
	/// the case for a chunked response and for one Faith decodes.
	pub content_length: Option<i64>,
}

/// The `Response` interface of the Fetch API represents the response to a request.
///
/// Faith does not allow its `Response` object to be constructed. If you need to, you may use the
/// `webResponse()` method to convert one into a Web API `Response` object; note the caveats.
#[napi]
#[derive(Debug, Clone)]
pub struct FaithResponse {
	pub(crate) inner: Response,
}

impl From<Response> for FaithResponse {
	fn from(inner: Response) -> Self {
		Self { inner }
	}
}

impl From<&ToFileOptions> for FileDestination {
	fn from(options: &ToFileOptions) -> Self {
		Self {
			overwrite: options.overwrite.unwrap_or(false),
			mode: options.mode,
		}
	}
}

impl From<FileProgress> for ToFileProgress {
	fn from(progress: FileProgress) -> Self {
		let count = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
		Self {
			bytes_written: count(progress.bytes_written),
			content_length: progress.content_length.map(count),
		}
	}
}

impl From<FileWritten> for ToFileResult {
	fn from(written: FileWritten) -> Self {
		Self {
			path: written.path,
			bytes_written: i64::try_from(written.bytes_written).unwrap_or(i64::MAX),
		}
	}
}

/// The callback `toFile()` reports progress to.
///
/// `CalleeHandled = false`: progress is not an error-first callback, so the JavaScript
/// side receives the report on its own rather than as the second argument.
pub type ProgressCallback =
	ThreadsafeFunction<ToFileProgress, Unknown<'static>, ToFileProgress, Status, false>;

#[napi]
impl FaithResponse {
	/// The `headers` read-only property of the `Response` interface contains the `Headers` object
	/// associated with the response.
	///
	/// Note that Faith does not provide a custom `Headers` class; instead the Web API `Headers` structure
	/// is used directly and constructed by Faith when needed.
	///
	/// This is a function as an internal implementation detail and the wrapper makes it a property.
	#[napi]
	pub fn headers(&self) -> Vec<(String, String)> {
		self.inner
			.headers()
			.iter()
			.filter_map(|(name, value)| {
				value
					.to_str()
					.ok()
					.map(|v| (name.to_string(), v.to_string()))
			})
			.collect()
	}

	/// The `ok` read-only property of the `Response` interface contains a boolean stating whether the
	/// response was successful (status in the range 200-299) or not.
	#[napi(getter)]
	pub fn ok(&self) -> bool {
		self.inner.ok()
	}

	/// Custom to Faith.
	///
	/// The `peer` read-only property of the `Response` interface contains an object with information about
	/// the remote peer that sent this response:
	#[napi(getter, ts_return_type = "{ address?: string; certificate?: Buffer }")]
	pub fn peer<'env>(&self, env: &'env Env) -> Result<Object<'env>, napi::Error> {
		let mut obj = Object::new(env)?;
		obj.set(
			"address",
			self.inner.peer.address.map(|addr| addr.to_string()),
		)?;
		obj.set(
			"certificate",
			self.inner
				.peer
				.certificate
				.as_deref()
				.map(|cert| Buffer::from(cert)),
		)?;
		Ok(obj)
	}

	/// The `redirected` read-only property of the `Response` interface indicates whether or not the
	/// response is the result of a request you made which was redirected.
	///
	/// Note that by the time you read this property, the redirect will already have happened, and you
	/// cannot prevent it by aborting the fetch at this point.
	///
	/// One caveat specific to Faith: with the agent's `http3.upgradeFollowAdvertisedPort`
	/// enabled, HTTP/3 responses compare URLs ignoring the port, because the port
	/// was rewritten to the advertised one and would otherwise register as a
	/// redirect. A genuine redirect differing only in port therefore reads as
	/// `false` on those responses.
	#[napi(getter)]
	pub fn redirected(&self) -> bool {
		self.inner.redirected()
	}

	/// The `status` read-only property of the `Response` interface contains the HTTP status codes of the
	/// response. For example, 200 for success, 404 if the resource could not be found.
	///
	/// A value is `0` is returned for a response whose `type` is `opaque`, `opaqueredirect`, or `error`.
	#[napi(getter)]
	pub fn status(&self) -> u16 {
		self.inner.status().as_u16()
	}

	/// The `statusText` read-only property of the `Response` interface contains the status message
	/// corresponding to the HTTP status code in `Response.status`. For example, this would be `OK` for a
	/// status code `200`, `Continue` for `100`, `Not Found` for `404`.
	///
	/// Faith always returns the canonical status message for the code. In HTTP/1, servers can send
	/// custom status text, but that text is not surfaced here; in HTTP/2 and HTTP/3, custom status
	/// text is not supported at all. For status codes with no well-known message, this is an empty
	/// string.
	#[napi(getter)]
	pub fn status_text(&self) -> &'static str {
		self.inner.status_text()
	}

	/// The `type` read-only property of the `Response` interface contains the type of the response. The
	/// type determines whether scripts are able to access the response body and headers.
	///
	/// In Faith, this is always set to `basic`.
	#[napi(getter, js_name = "type")]
	pub fn typ(&self) -> &'static str {
		"basic"
	}

	/// The `url` read-only property of the `Response` interface contains the URL of the response. The
	/// value of the `url` property will be the final URL obtained after any redirects.
	#[napi(getter)]
	pub fn url(&self) -> String {
		self.inner.url().to_string()
	}

	/// The `version` read-only property of the `Response` interface contains the HTTP version of the
	/// response. The value will be the final HTTP version after any redirects and protocol upgrades.
	///
	/// This is custom to Faith.
	#[napi(getter)]
	pub fn version(&self) -> String {
		format!("{:?}", self.inner.version)
	}

	/// The `bodyUsed` read-only property of the `Response` interface is a boolean value that indicates
	/// whether the body has been read yet.
	///
	/// In Faith, this indicates whether the body stream has ever been read from or canceled, as defined
	/// [in the standard](https://streams.spec.whatwg.org/#is-readable-stream-disturbed). Note that accessing
	/// the `.body` property counts as a read, even if you don't actually consume any bytes of content.
	#[napi(getter)]
	pub fn body_used(&self) -> bool {
		self.inner.disturbed.load(Ordering::SeqCst)
	}

	/// The `body` read-only property of the `Response` interface is a `ReadableStream` of the body
	/// contents, or `null` for any actual HTTP response that has no body, such as `HEAD` requests and
	/// `204 No Content` responses.
	///
	/// Note that browsers currently do not return `null` for those responses, but the standard
	/// requires it. Faith chooses to respect the standard rather than the browsers in this case.
	///
	/// An important consideration exists in conjunction with the connection pool: if you start the
	/// body stream, this will hold the connection until the stream is fully consumed. If another
	/// request is started during that time, and you don't have an available connection in the pool
	/// for the host already, the new request will open one.
	///
	/// Note that this is a function as an implementation detail; the wrapper makes it a property.
	#[napi]
	pub fn body(
		&self,
		env: Env,
	) -> Result<Option<napi::bindgen_prelude::ReadableStream<'_, BufferSlice<'_>>>, napi::Error> {
		// we mark the body as disturbed, but we still allow reading it through here
		// as essentially, the body() can be accessed many times as the same stream
		let _ = self.inner.check_stream_disturbed();

		let Some(lock) = &self.inner.body.body else {
			return Ok(None);
		};

		// if the lock is taken then we're consuming the body somehow
		let mut body = lock
			.try_lock()
			.map_err(|_| FaithError::from(FaithErrorKind::ResponseAlreadyDisturbed).into_napi())?;

		let stream = self
			.inner
			.ensure_stream(&mut body, self.inner.body.drained.clone())
			.map_err(|e| e.into_napi())?;

		let stream = napi::bindgen_prelude::ReadableStream::create_with_stream_bytes(
			&env,
			stream
				.map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)).into_napi()),
		)
		.map_err(|e| {
			napi::Error::from(
				FaithError::new(FaithErrorKind::BodyStream, Some(e.to_string()))
					.into_js_error(&env),
			)
		})?;
		Ok(Some(stream))
	}

	/// Discard the response body, releasing the connection back to the pool.
	///
	/// This is useful when you don't need the body but want to ensure the connection
	/// can be reused for subsequent requests. If you don't call this and don't consume
	/// the body, the connection may be held open until the response is garbage collected.
	///
	/// For HTTP/1, the remaining body is read and thrown away so the connection can go back
	/// to the pool. For HTTP/2 and HTTP/3, the body is dropped instead, which cancels the
	/// stream (RST_STREAM / STOP_SENDING) without affecting the multiplexed connection.
	///
	/// Returns a promise that resolves when the body has been fully discarded.
	#[napi]
	pub fn discard<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, ()>, napi::Error> {
		let this = Clone::clone(self);
		faith_promise(env, async move {
			this.inner.discard().await;
			Ok(())
		})
	}

	/// The `bytes()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise that resolves with a `Uint8Array`.
	///
	/// In Faith, this returns a Node.js `Buffer`, which can be used as (and is a subclass of) a `Uint8Array`.
	#[napi]
	pub fn bytes<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, Buffer>, napi::Error> {
		let this = Clone::clone(self);
		faith_promise(
			env,
			async move { this.inner.bytes().await.map(Buffer::from) },
		)
	}

	/// The `text()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise that resolves with a `String`. The response is always decoded
	/// using UTF-8; as per the standard, invalid UTF-8 sequences are replaced with U+FFFD rather
	/// than causing an error.
	#[napi]
	pub fn text<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, String>, napi::Error> {
		let this = Clone::clone(self);
		faith_promise(env, async move { this.inner.text().await })
	}

	/// The `json()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise which resolves with the result of parsing the body text as
	/// `JSON`.
	///
	/// Note that despite the method being named `json()`, the result is not JSON but is instead the
	/// result of taking JSON as input and parsing it to produce a JavaScript object.
	///
	/// Further note that, at least in Faith, this method first reads the entire response body as bytes,
	/// and then parses that as JSON. This can use up to double the amount of memory. If you need more
	/// efficient access, consider handling the response body as a stream.
	#[napi]
	pub fn json<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, Value>, napi::Error> {
		let this = Clone::clone(self);
		faith_promise(env, async move { this.inner.json().await.map(Value) })
	}

	/// Custom to Faith.
	///
	/// `toFile(path, options)` writes the response body to a file on disk, the bytes
	/// travelling from the network to the filesystem inside Faith without crossing into
	/// JavaScript. It is a whole-body read alongside `bytes()` and its siblings: the first
	/// consumer wins, `bodyUsed` becomes true once the read begins, and `integrity` is
	/// verified when set.
	///
	/// Resolves to `{ path, bytesWritten }`, where `path` is the absolute filesystem path
	/// written to and `bytesWritten` counts the bytes that landed there.
	///
	/// `onProgress` is reported to as the bytes land, at most every
	/// `PROGRESS_INTERVAL`, with a final report once the last byte is written. The
	/// wrapper takes it from the options object; it arrives here as its own argument
	/// because a threadsafe function cannot be a field of a `#[napi(object)]`.
	///
	/// The `file://` URL to path conversion and the `InvalidPath` rejection happen in the
	/// wrapper, so this receives a resolved string path.
	///
	/// spec:BODY#tofile
	#[napi(
		ts_args_type = "path: string, options?: ToFileOptions | undefined | null, onProgress?: ((progress: ToFileProgress) => void) | undefined | null"
	)]
	pub fn to_file<'env>(
		&self,
		env: &'env Env,
		path: String,
		options: Option<ToFileOptions>,
		on_progress: Option<ProgressCallback>,
	) -> Result<PromiseRaw<'env, ToFileResult>, napi::Error> {
		let this = Clone::clone(self);
		let options = options.unwrap_or_default();
		faith_promise(env, async move {
			let destination = FileDestination::from(&options);
			let written = this
				.inner
				.write_to_file(&path, &destination, |progress| {
					if let Some(callback) = &on_progress {
						callback.call(
							ToFileProgress::from(progress),
							// Progress is observational: a report the queue cannot take is
							// dropped rather than made to hold up the write it describes.
							ThreadsafeFunctionCallMode::NonBlocking,
						);
					}
				})
				.await?;
			Ok(ToFileResult::from(written))
		})
	}

	/// Custom to Faith.
	///
	/// The measurements behind the `timing` property, which the wrapper turns into a
	/// `PerformanceResourceTiming`.
	///
	/// A resource timing entry describes a finished request, so this does not resolve until
	/// the body has ended: by being read, by `discard()`, or by the collector draining one
	/// that was abandoned. A response that cannot carry a body has ended already.
	///
	/// Phases are milliseconds from the start of the request rather than absolute times, so
	/// the wrapper can place them on the same clock as the platform's other performance
	/// entries.
	///
	/// This is an async fn as an internal implementation detail and the wrapper makes it a
	/// property.
	// spec:RESP#request-timing
	#[napi]
	pub async fn timing(&self) -> TimingBreakdown {
		self.inner.timing().await.into()
	}

	/// The `trailers()` read-only property of the `Response` interface returns a promise that
	/// resolves to either `null` or a `Headers` structure that contains the HTTP/2 or /3 trailing
	/// headers.
	///
	/// This was once in the standard as a getter, but was removed as no browser implemented it.
	///
	/// Trailers only exist once the body has ended, so this does not resolve until the body
	/// has been consumed — by `text()`, `bytes()`, `json()`, `blob()`, or reading the `body`
	/// stream. Awaiting it first, on its own, waits forever: that is the behaviour the fetch
	/// standard's trailers proposal describes (<https://github.com/whatwg/fetch/pull/1940>), not
	/// a quirk of Faith. Holding the promise while something else reads the body is fine, and
	/// costs nothing while it is pending.
	///
	/// `discard()` counts as consuming the body but discards its trailers with it, so this
	/// then resolves to `null` rather than waiting for trailers that can no longer arrive.
	///
	/// This is an async fn as an internal implementation detail and the wrapper makes it a property.
	#[napi]
	pub async fn trailers(&self) -> Option<Vec<(String, String)>> {
		match self.inner.trailers().await {
			// NotYet cannot come back from `settled`, which is what it waits on.
			Trailers::NotYet | Trailers::None => None,
			Trailers::Some(headers) => Some(
				headers
					.iter()
					.filter_map(|(name, value)| {
						value
							.to_str()
							.ok()
							.map(|v| (name.to_string(), v.to_string()))
					})
					.collect(),
			),
		}
	}

	/// The `clone()` method of the `Response` interface creates a clone of a response object, identical
	/// in every way, but stored in a different variable.
	///
	/// `clone()` throws an `Error` if the response body has already been used.
	///
	/// (Per the standard, this should throw a `TypeError`, but for technical reasons this is not
	/// possible with Faith.)
	#[napi]
	pub fn clone(&self, env: Env) -> Result<Self, napi::Error> {
		if self.inner.disturbed.load(Ordering::SeqCst) {
			return Err(FaithError::from(FaithErrorKind::ResponseAlreadyDisturbed)
				.into_js_error(&env)
				.into());
		}

		Ok(Self::from(Response {
			disturbed: Arc::new(AtomicBool::new(false)),
			..Clone::clone(&self.inner)
		}))
	}
}
