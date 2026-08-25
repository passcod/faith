//! Sending a request, and building the response that comes back.

// spec:REQ spec:ENC spec:CANCEL

mod builder;
mod send;
mod target;

pub use builder::{FetchBuilder, Priority, Request, RequestBuilder};
pub use send::send;
pub use target::Target;

use std::{pin::Pin, time::Duration};

use bytes::Bytes;
use futures::Stream;

#[cfg(feature = "cache")]
use http_cache_reqwest::CacheMode;

/// Whether a request carries its credentials, and how far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Credentials {
	/// Strip credentials from the URL and send no cookies.
	Omit,
	/// Send them, which is what a server-side caller almost always means.
	#[default]
	Include,
}

/// The methods the fetch standard normalises to upper case; any other method is sent as given.
// spec:REQ#method-and-headers
const NORMALISED_METHODS: [&str; 6] = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"];

/// The header a request's priority is expressed in.
// spec:REQ#request-priority
pub const PRIORITY: &str = "priority";

/// A request body, as the caller has it.
pub enum RequestBody {
	/// No body.
	None,
	/// A body already in hand, whose length can be declared up front.
	Bytes(Bytes),
	/// A body arriving in chunks, which goes out chunked because it has no length to declare.
	Stream(Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>),
}

/// What a request carries beyond its method, URL, and body.
#[derive(Clone, Debug, Default)]
pub struct RequestOptions {
	#[cfg(feature = "cache")]
	pub cache: CacheMode,
	/// A coding to compress the body in, named by its wire token.
	#[cfg(feature = "encoding")]
	pub compress: Option<String>,
	pub credentials: Credentials,
	pub headers: Option<Vec<(String, String)>>,
	pub integrity: Option<String>,
	pub method: Option<String>,
	/// The `Priority` header value this request's priority derives, if it derives one.
	pub priority: Option<&'static str>,
	pub timeout: Option<Duration>,
}
