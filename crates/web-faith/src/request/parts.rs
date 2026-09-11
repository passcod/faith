//! The request shapes an `internals` caller constructs directly.
//!
//! In their own module so the feature can decide whether they are public.

use std::{pin::Pin, time::Duration};

use bytes::Bytes;
use futures::Stream;

#[cfg(feature = "cache")]
use http_cache_reqwest::CacheMode;

use super::Credentials;

/// A request body, as the caller has it.
pub enum RequestBody {
	/// No body.
	None,
	/// A body already in hand, whose length can be declared up front.
	Bytes(Bytes),
	/// A body arriving in chunks, which goes out chunked because it has no length to declare.
	Stream(Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>),
}

/// The settings a request carries beyond its method, URL, and body.
#[derive(Clone, Debug, Default)]
pub struct RequestOptions {
	#[cfg(feature = "cache")]
	/// How the HTTP cache is consulted.
	pub cache: CacheMode,
	/// A coding to compress the body in, named by its wire token.
	#[cfg(feature = "encoding")]
	pub compress: Option<String>,
	/// The `Content-Type` the request body's kind implies, per the fetch standard's body
	/// extraction. Faith sends it only when nothing else declares a type, so a header on the
	/// request or a default on the agent both win over it.
	// spec:REQ#body
	pub body_content_type: Option<String>,
	/// Whether the request carries its credentials.
	pub credentials: Credentials,
	/// Headers to send, merged over the agent's defaults by name.
	pub headers: Option<Vec<(String, String)>>,
	/// Digests the response body is expected to match.
	pub integrity: Option<String>,
	/// The method. `GET` when unset.
	pub method: Option<String>,
	/// The `Priority` header value this request's priority derives, if it derives one.
	pub priority: Option<&'static str>,
	/// A deadline for the whole request and response.
	pub timeout: Option<Duration>,
}
