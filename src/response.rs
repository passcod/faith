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
use reqwest::{StatusCode, Url, Version, header::HeaderMap};
use serde_json;
use stream_shared::SharedStream;
use tokio::{
	sync::{Mutex, RwLock},
	task::yield_now,
};

use crate::{
	agent::InnerAgentStats,
	async_task::{Async, FaithAsyncResult, Value},
	body::{Body, DynStream},
	error::{FaithError, FaithErrorKind},
};

/// The `Response` interface of the Fetch API represents the response to a request.
///
/// Fáith does not allow its `Response` object to be constructed. If you need to, you may use the
/// `webResponse()` method to convert one into a Web API `Response` object; note the caveats.
#[napi]
#[derive(Debug, Clone)]
pub struct FaithResponse {
	pub(crate) body: Option<Arc<Mutex<Body>>>,
	pub(crate) disturbed: Arc<AtomicBool>,
	pub(crate) headers: HeaderMap,
	pub(crate) peer: Arc<PeerInformation>,
	pub(crate) redirected: bool,
	pub(crate) stats: Arc<InnerAgentStats>,
	pub(crate) status_code: StatusCode,
	pub(crate) trailers: Arc<RwLock<Trailers>>,
	pub(crate) url: Url,
	pub(crate) version: Version,
}

/// Custom to Fáith.
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

#[derive(Debug, Default)]
pub enum Trailers {
	#[default]
	NotYet,
	None,
	Some(HeaderMap),
}

#[napi]
impl FaithResponse {
	/// The `headers` read-only property of the `Response` interface contains the `Headers` object
	/// associated with the response.
	///
	/// Note that Fáith does not provide a custom `Headers` class; instead the Web API `Headers` structure
	/// is used directly and constructed by Fáith when needed.
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

	/// Custom to Fáith.
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
	/// In HTTP/1, servers can send custom status text. This is returned here. In HTTP/2 and HTTP/3, custom
	/// status text is not supported at all, and the `statusText` property is either empty or simulated
	/// from well-known status codes.
	#[napi(getter)]
	pub fn status_text(&self) -> &'static str {
		self.status_code.canonical_reason().unwrap_or_default()
	}

	/// The `type` read-only property of the `Response` interface contains the type of the response. The
	/// type determines whether scripts are able to access the response body and headers.
	///
	/// In Fáith, this is always set to `basic`.
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
	/// This is custom to Fáith.
	#[napi(getter)]
	pub fn version(&self) -> String {
		format!("{:?}", self.version)
	}

	/// The `bodyUsed` read-only property of the `Response` interface is a boolean value that indicates
	/// whether the body has been read yet.
	///
	/// In Fáith, this indicates whether the body stream has ever been read from or canceled, as defined
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
	/// it. Fáith chooses to respect the spec rather than the browsers in this case.
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

		let Some(lock) = &self.body else {
			return Ok(None);
		};

		// if the lock is taken then we're consuming the body somehow
		let mut body = lock
			.try_lock()
			.map_err(|_| FaithError::from(FaithErrorKind::ResponseAlreadyDisturbed).into_napi())?;

		let stream = self.ensure_stream(&mut body).map_err(|e| e.into_napi())?;

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
				let stream = SharedStream::new(Box::pin(
					BodyStream::new(inner)
						.then(move |frame| {
							let trailers_lock = trailers_stream.clone();
							async move {
								match frame {
									Err(err) => Some(Err(err.to_string())),
									Ok(frame) => match frame.into_trailers() {
										Ok(trailers) => {
											let mut t = trailers_lock.write().await;
											*t = Trailers::Some(trailers);
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
						.chain(stream::once(async move {
							let mut t = trailers_finish.write().await;
							if matches!(*t, Trailers::NotYet) {
								*t = Trailers::None;
							}
							// Track that we've finished consuming a body
							stats_finish.bodies_finished.fetch_add(1, Ordering::Relaxed);
							None
						}))
						.filter_map(async |item| item),
				) as Pin<Box<DynStream>>);

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
		let Some(lock) = &self.body else {
			return Ok(Default::default());
		};

		let mut body = lock.lock().await;
		let stream = self.ensure_stream(&mut body)?;
		drop(body); // release lock before consuming stream

		let mut chunks = Vec::new();
		futures::pin_mut!(stream);
		while let Some(result) = stream.next().await {
			let chunk =
				result.map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)))?;
			chunks.push(chunk);
		}

		Ok(Arc::from(chunks.into_boxed_slice()))
	}

	/// gather() and then copy into one contiguous buffer
	async fn gather_contiguous(&self) -> Result<Buffer, FaithError> {
		let body = self.gather().await?;
		let length = body.iter().map(|chunk| chunk.len()).sum();
		let mut bytes = Vec::with_capacity(length);
		for chunk in body.into_iter() {
			bytes.extend_from_slice(chunk);
		}
		Ok(bytes.into())
	}

	/// The `bytes()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise that resolves with a `Uint8Array`.
	///
	/// In Fáith, this returns a Node.js `Buffer`, which can be used as (and is a subclass of) a `Uint8Array`.
	#[napi]
	pub fn bytes(&self) -> Async<Buffer> {
		let this = Clone::clone(&*self);
		FaithAsyncResult::run(async move || {
			this.check_stream_disturbed()?;
			let buf = this.gather_contiguous().await?;
			Ok(buf)
		})
	}

	/// The `text()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise that resolves with a `String`. The response is always decoded
	/// using UTF-8.
	#[napi]
	pub fn text(&self) -> Async<String> {
		let this = Clone::clone(&*self);
		FaithAsyncResult::run(async move || {
			this.check_stream_disturbed()?;
			let bytes = this.gather_contiguous().await?;
			String::from_utf8(bytes.to_vec())
				.map_err(|e| FaithError::new(FaithErrorKind::Utf8Parse, Some(e.to_string())).into())
		})
	}

	/// The `json()` method of the `Response` interface takes a `Response` stream and reads it to
	/// completion. It returns a promise which resolves with the result of parsing the body text as
	/// `JSON`.
	///
	/// Note that despite the method being named `json()`, the result is not JSON but is instead the
	/// result of taking JSON as input and parsing it to produce a JavaScript object.
	///
	/// Further note that, at least in Fáith, this method first reads the entire response body as bytes,
	/// and then parses that as JSON. This can use up to double the amount of memory. If you need more
	/// efficient access, consider handling the response body as a stream.
	#[napi]
	pub fn json(&self) -> Async<Value> {
		let this = Clone::clone(&*self);
		FaithAsyncResult::run(async move || {
			this.check_stream_disturbed()?;
			let bytes = this.gather_contiguous().await?;
			let value = serde_json::from_slice(&bytes)
				.map_err(|e| FaithError::new(FaithErrorKind::JsonParse, Some(e.to_string())))?;
			Ok(Value(value))
		})
	}

	/// The `trailers()` read-only property of the `Response` interface returns a promise that
	/// resolves to either `null` or a `Headers` structure that contains the HTTP/2 or /3 trailing
	/// headers.
	///
	/// This was once in the spec as a getter but was removed as it wasn't implemented by any browser.
	///
	/// Note that this will never resolve if you don't also consume the body in some way.
	///
	/// This is an async fn as an internal implementation detail and the wrapper makes it a property.
	#[napi]
	pub async fn trailers(&self) -> Option<Vec<(String, String)>> {
		let t = Arc::clone(&self.trailers);
		loop {
			match &*t.read().await {
				Trailers::NotYet => {
					yield_now().await;
					continue;
				}
				Trailers::None => break None,
				Trailers::Some(h) => {
					break Some(
						h.iter()
							.filter_map(|(name, value)| {
								value
									.to_str()
									.ok()
									.map(|v| (name.to_string(), v.to_string()))
							})
							.collect(),
					);
				}
			}
		}
	}

	/// The `clone()` method of the `Response` interface creates a clone of a response object, identical
	/// in every way, but stored in a different variable.
	///
	/// `clone()` throws an `Error` if the response body has already been used.
	///
	/// (In-spec, this should throw a `TypeError`, but for technical reasons this is not possible with Fáith.)
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
