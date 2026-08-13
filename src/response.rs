use std::{
	fmt::Debug,
	hint::unreachable_unchecked,
	mem::replace,
	net::SocketAddr,
	pin::Pin,
	result::Result,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream};
use http_body_util::BodyStream;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use reqwest::{
	StatusCode, Url, Version,
	header::{CONTENT_LENGTH, HeaderMap},
};
use serde_json;
use stream_shared::SharedStream;
use tokio::{io::AsyncWriteExt, sync::watch};

use crate::{
	agent::InnerAgentStats,
	async_task::{Value, faith_promise},
	body::{Body, BodyHolder, DynStream, drain_body_inner},
	encoding::{Coding, decode_stream},
	error::{FaithError, FaithErrorKind},
	integrity::{finish_integrity, integrity_checker, verify_integrity},
	timing::{TimingBreakdown, TimingSlot},
};

/// The `Response` interface of the Fetch API represents the response to a request.
///
/// Faith does not allow its `Response` object to be constructed. If you need to, you may use the
/// `webResponse()` method to convert one into a Web API `Response` object; note the caveats.
#[napi]
#[derive(Debug, Clone)]
pub struct FaithResponse {
	pub(crate) body: BodyHolder,
	/// The coding to decode the body under, or `None` to deliver it as received.
	/// Set once when the response is built, from the request's `Accept-Encoding` and the
	/// response's `Content-Encoding` (see [`crate::encoding`]).
	pub(crate) decode: Option<Coding>,
	pub(crate) disturbed: Arc<AtomicBool>,
	pub(crate) headers: HeaderMap,
	pub(crate) integrity: Option<String>,
	pub(crate) peer: Arc<PeerInformation>,
	pub(crate) redirected: bool,
	pub(crate) stats: Arc<InnerAgentStats>,
	pub(crate) status_code: StatusCode,
	pub(crate) timing: Arc<TimingSlot>,
	pub(crate) trailers: Arc<TrailersSlot>,
	pub(crate) url: Url,
	pub(crate) version: Version,
}

/// Custom to Faith.
///
/// The `peer` read-only property of the `Response` interface contains an object with information about
/// the remote peer that sent this response:
///
/// - `address`: The IP address and port of the peer, if available.
/// - `certificate`: When connected over HTTPS, this is the DER-encoded leaf certificate of the peer.
#[derive(Debug)]
pub struct PeerInformation {
	pub address: Option<SocketAddr>,
	pub certificate: Option<Vec<u8>>,
}

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

/// Open the destination file for a body write, mapping filesystem refusals to the errors
/// `toFile()` surfaces (spec:BODY#tofile).
async fn open_destination(
	path: &str,
	options: &ToFileOptions,
) -> Result<tokio::fs::File, FaithError> {
	let mut open = tokio::fs::OpenOptions::new();
	open.write(true);
	if options.overwrite.unwrap_or(false) {
		// An occupied destination is truncated and replaced.
		open.create(true).truncate(true);
	} else {
		// The safe default refuses an occupied destination outright.
		open.create_new(true);
	}
	#[cfg(unix)]
	if let Some(mode) = options.mode {
		open.mode(mode);
	}

	match open.open(path).await {
		Ok(file) => Ok(file),
		Err(err) => Err(classify_open_error(path, err).await),
	}
}

/// Classify a failure to open the destination. An occupied destination is `FileExists`,
/// unless what occupies it is a directory: a directory is well-formed but cannot be written
/// to, which is a `FileWrite`. Every other refusal is a `FileWrite` carrying the OS detail.
async fn classify_open_error(path: &str, err: std::io::Error) -> FaithError {
	let kind = if err.kind() == std::io::ErrorKind::AlreadyExists {
		match tokio::fs::symlink_metadata(path).await {
			Ok(meta) if meta.is_dir() => FaithErrorKind::FileWrite,
			_ => FaithErrorKind::FileExists,
		}
	} else {
		FaithErrorKind::FileWrite
	};
	FaithError::new(kind, Some(err.to_string()))
}

#[derive(Clone, Debug, Default)]
pub enum Trailers {
	#[default]
	NotYet,
	None,
	Some(HeaderMap),
}

/// Where the trailers land: written by whoever finishes the body, awaited by `trailers()`.
///
/// A watch channel, rather than a lock read in a loop. Per the fetch spec's trailers
/// proposal (<https://github.com/whatwg/fetch/pull/1940>) this promise is *meant* not to
/// resolve until the body has been consumed, so the wait is unbounded by design -- which is
/// precisely why polling was the wrong shape for it. Awaiting trailers without reading the
/// body now leaves an idle pending promise rather than a pegged core, and the future can be
/// cancelled while it waits.
#[derive(Debug)]
pub struct TrailersSlot(watch::Sender<Trailers>);

impl Default for TrailersSlot {
	fn default() -> Self {
		Self(watch::channel(Trailers::NotYet).0)
	}
}

impl TrailersSlot {
	/// Record trailers that arrived, waking whoever is waiting.
	fn arrived(&self, trailers: HeaderMap) {
		self.0.send_replace(Trailers::Some(trailers));
	}

	/// Record that the body ended, if no trailers frame got there first.
	///
	/// `send_if_modified` so the read and the write are one step, and so waiters are woken
	/// only by the call that actually settled it.
	fn ended(&self) {
		self.0.send_if_modified(|state| {
			if matches!(state, Trailers::NotYet) {
				*state = Trailers::None;
				true
			} else {
				false
			}
		});
	}

	/// Wait until the body has settled the question.
	async fn settled(&self) -> Trailers {
		let mut rx = self.0.subscribe();
		// `wait_for` tests the current value before waiting, so trailers that already
		// arrived return without yielding. Its error case is the sender being gone, which
		// means the response was dropped and nothing can ever set this -- no trailers is
		// the only answer left.
		match rx
			.wait_for(|state| !matches!(state, Trailers::NotYet))
			.await
		{
			Ok(state) => state.clone(),
			Err(_) => Trailers::None,
		}
	}
}

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
		self.headers
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
		self.status_code.is_success()
	}

	/// Custom to Faith.
	///
	/// The `peer` read-only property of the `Response` interface contains an object with information about
	/// the remote peer that sent this response:
	#[napi(getter, ts_return_type = "{ address?: string; certificate?: Buffer }")]
	pub fn peer<'env>(&self, env: &'env Env) -> Result<Object<'env>, napi::Error> {
		let mut obj = Object::new(env)?;
		obj.set("address", self.peer.address.map(|addr| addr.to_string()))?;
		obj.set(
			"certificate",
			self.peer
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
		self.redirected
	}

	/// The `status` read-only property of the `Response` interface contains the HTTP status codes of the
	/// response. For example, 200 for success, 404 if the resource could not be found.
	///
	/// A value is `0` is returned for a response whose `type` is `opaque`, `opaqueredirect`, or `error`.
	#[napi(getter)]
	pub fn status(&self) -> u16 {
		self.status_code.as_u16()
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
		self.status_code.canonical_reason().unwrap_or_default()
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
		self.url.to_string()
	}

	/// The `version` read-only property of the `Response` interface contains the HTTP version of the
	/// response. The value will be the final HTTP version after any redirects and protocol upgrades.
	///
	/// This is custom to Faith.
	#[napi(getter)]
	pub fn version(&self) -> String {
		format!("{:?}", self.version)
	}

	/// The `bodyUsed` read-only property of the `Response` interface is a boolean value that indicates
	/// whether the body has been read yet.
	///
	/// In Faith, this indicates whether the body stream has ever been read from or canceled, as defined
	/// [in the spec](https://streams.spec.whatwg.org/#is-readable-stream-disturbed). Note that accessing
	/// the `.body` property counts as a read, even if you don't actually consume any bytes of content.
	#[napi(getter)]
	pub fn body_used(&self) -> bool {
		self.disturbed.load(Ordering::SeqCst)
	}

	/// The `body` read-only property of the `Response` interface is a `ReadableStream` of the body
	/// contents, or `null` for any actual HTTP response that has no body, such as `HEAD` requests and
	/// `204 No Content` responses.
	///
	/// Note that browsers currently do not return `null` for those responses, but the spec requires
	/// it. Faith chooses to respect the spec rather than the browsers in this case.
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
		let _ = self.check_stream_disturbed();

		let Some(lock) = &self.body.body else {
			return Ok(None);
		};

		// if the lock is taken then we're consuming the body somehow
		let mut body = lock
			.try_lock()
			.map_err(|_| FaithError::from(FaithErrorKind::ResponseAlreadyDisturbed).into_napi())?;

		let stream = self
			.ensure_stream(&mut body, self.body.drained.clone())
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

	fn check_stream_disturbed(&self) -> Result<(), FaithError> {
		if self.disturbed.swap(true, Ordering::SeqCst) {
			Err(FaithErrorKind::ResponseAlreadyDisturbed.into())
		} else {
			Ok(())
		}
	}

	/// Ensures the body is converted to a SharedStream, returning a clone of it.
	///
	/// This allows multiple consumers (original + clones) to independently read the body.
	fn ensure_stream(
		&self,
		body: &mut Body,
		drained_flag: Arc<AtomicBool>,
	) -> Result<SharedStream<Pin<Box<DynStream>>>, FaithError> {
		match body {
			Body::Consumed => Err(FaithErrorKind::ResponseAlreadyDisturbed.into()),
			Body::Stream(stream) => Ok(stream.clone()),
			lock @ Body::Inner(_) => {
				// temporarily replace with Consumed until we can put in the Stream
				let Body::Inner(inner) = replace(lock, Body::Consumed) else {
					// SAFETY: we're inside the match checking for this exact thing
					unsafe { unreachable_unchecked() }
				};

				// Track that we've started consuming a body
				self.stats.bodies_started.fetch_add(1, Ordering::Relaxed);

				let trailers_stream = self.trailers.clone();
				let trailers_finish = self.trailers.clone();
				let stats_finish = self.stats.clone();
				let timing_finish = self.timing.clone();
				let drained_finish = drained_flag.clone();
				// The frame stream pulls trailers off to the side (via `arrived`) and yields
				// data bytes only, so decoding sees no trailer frames.
				let bytes = Box::pin(
					BodyStream::new(inner)
						.then(move |frame| {
							let trailers_lock = trailers_stream.clone();
							async move {
								match frame {
									Err(err) => Some(Err(err.to_string())),
									Ok(frame) => match frame.into_trailers() {
										Ok(trailers) => {
											trailers_lock.arrived(trailers);
											None
										}
										Err(frame) => Some(
											frame
												.into_data()
												.map_err(|_| "unknown frame kind".to_string()),
										),
									},
								}
							}
						})
						.filter_map(async |item| item),
				) as Pin<Box<DynStream>>;

				let bytes = match self.decode {
					Some(coding) => decode_stream(bytes, coding),
					None => bytes,
				};

				// Chained onto the stream that is actually delivered, above any decoder: a
				// decoder reaches the end of its own framing without necessarily polling the
				// bytes underneath to completion, so bookkeeping chained below it would never
				// run for a decoded body, leaving the trailers promise and the timing pending
				// for good.
				let bytes = Box::pin(
					bytes.chain(
						stream::once(async move {
							trailers_finish.ended();
							// The last byte of the body: every read path ends here, so
							// this is where the timing settles
							// (spec:RESP#request-timing).
							timing_finish.ended();
							// Track that we've finished consuming a body
							stats_finish.bodies_finished.fetch_add(1, Ordering::Relaxed);
							// Mark body as drained so Drop doesn't try to drain again
							drained_finish.store(true, Ordering::SeqCst);
						})
						.filter_map(async |()| None),
					),
				) as Pin<Box<DynStream>>;

				let stream = SharedStream::new(bytes);

				// the _ is the Consumed we put in there earlier
				let _ = replace(lock, Body::Stream(stream.clone()));

				Ok(stream)
			}
		}
	}

	/// Underlying efficient response body fetcher.
	///
	/// Unlike bytes() and co, this grabs all the chunks of the response but doesn't
	/// copy them. Further processing is needed to obtain a Vec<u8> or whatever needed.
	async fn gather(&self) -> Result<Arc<[Bytes]>, FaithError> {
		let Some(lock) = &self.body.body else {
			return Ok(Default::default());
		};

		let mut body = lock.lock().await;
		let stream = self.ensure_stream(&mut body, self.body.drained.clone())?;
		drop(body); // release lock before consuming stream

		let mut chunks = Vec::new();
		futures::pin_mut!(stream);
		while let Some(result) = stream.next().await {
			let chunk =
				result.map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)))?;
			chunks.push(chunk);
		}

		// Mark as drained since we consumed everything
		self.body.mark_drained();

		Ok(Arc::from(chunks.into_boxed_slice()))
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
		let body = self.body.body.clone();
		let drained_flag = self.body.drained.clone();
		let is_multiplexed = self.body.is_multiplexed();
		let trailers = self.trailers.clone();
		let timing = self.timing.clone();
		faith_promise(env, async move {
			if let Some(arc) = body {
				if is_multiplexed {
					// Multiplexed connections don't need draining for reuse; dropping
					// the body cancels the stream and frees its resources right away
					// instead of waiting for garbage collection.
					let mut guard = arc.lock().await;
					*guard = Body::Consumed;
				} else {
					drain_body_inner(arc).await;
				}
			}
			drained_flag.store(true, Ordering::SeqCst);
			// Discarding the body discards its trailers: on a multiplexed connection the
			// stream was cancelled before any could arrive, and draining an HTTP/1 body
			// here bypasses the stream that would have collected them. Settling this as
			// "none" rather than leaving it pending is the point -- a caller who discarded
			// the body and then awaited trailers used to wait forever.
			trailers.ended();
			// Discarding is one of the ways a body finishes (spec:RESP#request-timing).
			timing.ended();
			Ok(())
		})
	}

	/// gather() and then copy into one contiguous buffer
	async fn gather_contiguous(&self) -> Result<Vec<u8>, FaithError> {
		let body = self.gather().await?;
		let length = body.iter().map(|chunk| chunk.len()).sum();
		let mut bytes = Vec::with_capacity(length);
		for chunk in body.into_iter() {
			bytes.extend_from_slice(chunk);
		}

		if let Some(ref integrity) = self.integrity {
			verify_integrity(&bytes, integrity)?;
		}

		Ok(bytes)
	}

	/// The `bytes()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise that resolves with a `Uint8Array`.
	///
	/// In Faith, this returns a Node.js `Buffer`, which can be used as (and is a subclass of) a `Uint8Array`.
	#[napi]
	pub fn bytes<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, Buffer>, napi::Error> {
		let this = Clone::clone(self);
		faith_promise(env, async move {
			this.check_stream_disturbed()?;
			this.gather_contiguous().await.map(Buffer::from)
		})
	}

	/// The `text()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise that resolves with a `String`. The response is always decoded
	/// using UTF-8; as per spec, invalid UTF-8 sequences are replaced with U+FFFD rather than
	/// causing an error.
	#[napi]
	pub fn text<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, String>, napi::Error> {
		let this = Clone::clone(self);
		faith_promise(env, async move {
			this.check_stream_disturbed()?;
			let bytes = this.gather_contiguous().await?;
			Ok(String::from_utf8(bytes)
				.unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
		})
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
		faith_promise(env, async move {
			this.check_stream_disturbed()?;
			let bytes = this.gather_contiguous().await?;
			let value = serde_json::from_slice(&bytes)
				.map_err(|e| FaithError::new(FaithErrorKind::JsonParse, Some(e.to_string())))?;
			Ok(Value(value))
		})
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
	/// The `file://` URL to path conversion and the `InvalidPath` rejection happen in the
	/// wrapper, so this receives a resolved string path.
	///
	/// spec:BODY#tofile
	#[napi]
	pub fn to_file<'env>(
		&self,
		env: &'env Env,
		path: String,
		options: Option<ToFileOptions>,
	) -> Result<PromiseRaw<'env, ToFileResult>, napi::Error> {
		let this = Clone::clone(self);
		let options = options.unwrap_or_default();
		faith_promise(env, async move { this.write_to_file(path, options).await })
	}

	async fn write_to_file(
		&self,
		path: String,
		options: ToFileOptions,
	) -> Result<ToFileResult, FaithError> {
		// A response that cannot carry a body has nothing to write, and this is settled
		// before any file is created (spec:BODY#tofile).
		let Some(lock) = self.body.body.clone() else {
			return Err(FaithErrorKind::ResponseBodyNull.into());
		};

		// A body already read, or whose stream was handed out, has no second read to give.
		// Checked without committing so an open failure below still leaves the body
		// undisturbed and the caller free to retry to another path.
		if self.disturbed.load(Ordering::SeqCst) {
			return Err(FaithErrorKind::ResponseAlreadyDisturbed.into());
		}

		// Reject a malformed integrity value up front, before the body is touched, the same
		// as the other verified reads reject it when the whole body is in hand.
		let mut checker = integrity_checker(self.integrity.as_deref())?;

		// The advertised length, when the server sent one. It is only visible here for a body
		// delivered as received: a decoded body has had its Content-Length stripped, so the
		// bytes written equal the wire bytes wherever this is Some (spec:BODY#tofile, ENC).
		let content_length = self
			.headers
			.get(CONTENT_LENGTH)
			.and_then(|value| value.to_str().ok())
			.and_then(|value| value.trim().parse::<u64>().ok());

		// The destination is opened before any of the body is read, so a failure to open it
		// leaves the body unread and undisturbed.
		let mut file = open_destination(&path, &options).await?;

		// Commit the read now the destination is in hand. A concurrent read that slipped in
		// since the load above wins, and this one finds the body already spent.
		self.check_stream_disturbed()?;

		let stream = {
			let mut body = lock.lock().await;
			let stream = self.ensure_stream(&mut body, self.body.drained.clone())?;
			drop(body); // release lock before consuming stream
			stream
		};

		let mut written: u64 = 0;
		futures::pin_mut!(stream);
		while let Some(result) = stream.next().await {
			let chunk =
				result.map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)))?;
			if let Some(checker) = checker.as_mut() {
				checker.input(&chunk);
			}
			file.write_all(&chunk)
				.await
				.map_err(|err| FaithError::new(FaithErrorKind::FileWrite, Some(err.to_string())))?;
			written += chunk.len() as u64;
			// A server cannot send more than it promised: once the bytes off the wire exceed
			// the advertised length, the write fails and the bytes so far stay on disk
			// (spec:BODY#tofile).
			if let Some(limit) = content_length {
				if written > limit {
					return Err(FaithErrorKind::ContentLengthOverrun.into());
				}
			}
		}

		file.flush()
			.await
			.map_err(|err| FaithError::new(FaithErrorKind::FileWrite, Some(err.to_string())))?;

		// The digest is only known once the last byte has been written, so the file that
		// fails verification is on disk when the error arrives (spec:SRI).
		if let Some(checker) = checker {
			finish_integrity(checker)?;
		}

		self.body.mark_drained();

		Ok(ToFileResult {
			// A relative path resolves against the process's working directory; the caller
			// is handed the absolute path the bytes landed at.
			path: std::path::absolute(&path)
				.map(|abs| abs.to_string_lossy().into_owned())
				.unwrap_or(path),
			bytes_written: written as i64,
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
	///
	/// spec:RESP#request-timing
	#[napi]
	pub async fn timing(&self) -> TimingBreakdown {
		self.timing.settled().await.into()
	}

	/// The `trailers()` read-only property of the `Response` interface returns a promise that
	/// resolves to either `null` or a `Headers` structure that contains the HTTP/2 or /3 trailing
	/// headers.
	///
	/// This was once in the spec as a getter but was removed as it wasn't implemented by any browser.
	///
	/// Trailers only exist once the body has ended, so this does not resolve until the body
	/// has been consumed — by `text()`, `bytes()`, `json()`, `blob()`, or reading the `body`
	/// stream. Awaiting it first, on its own, waits forever: that is the behaviour the fetch
	/// spec's trailers proposal describes (<https://github.com/whatwg/fetch/pull/1940>), not
	/// a quirk of Faith. Holding the promise while something else reads the body is fine, and
	/// costs nothing while it is pending.
	///
	/// `discard()` counts as consuming the body but discards its trailers with it, so this
	/// then resolves to `null` rather than waiting for trailers that can no longer arrive.
	///
	/// This is an async fn as an internal implementation detail and the wrapper makes it a property.
	#[napi]
	pub async fn trailers(&self) -> Option<Vec<(String, String)>> {
		match self.trailers.settled().await {
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
	/// (In-spec, this should throw a `TypeError`, but for technical reasons this is not possible with Faith.)
	#[napi]
	pub fn clone(&self, env: Env) -> Result<Self, napi::Error> {
		if self.disturbed.load(Ordering::SeqCst) {
			return Err(FaithError::from(FaithErrorKind::ResponseAlreadyDisturbed)
				.into_js_error(&env)
				.into());
		}

		Ok(Self {
			disturbed: Arc::new(AtomicBool::new(false)),
			..Clone::clone(self)
		})
	}
}
