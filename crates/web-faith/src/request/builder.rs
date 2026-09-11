use std::{
	future::{Future, IntoFuture},
	pin::Pin,
	time::Duration,
};

use bytes::Bytes;
use futures::Stream;

#[cfg(feature = "cache")]
use http_cache_reqwest::CacheMode;
use reqwest::{
	Method,
	header::{HeaderName, HeaderValue},
};
use url::Url;

use crate::{
	agent::Agent,
	error::{FaithError, FaithErrorKind},
	request::{Credentials, RequestBody, RequestOptions, Target, send::send},
	response::Response,
};

/// A header the caller set or removed on one layer.
#[derive(Clone, Debug)]
struct HeaderOp {
	name: String,
	/// `None` removes the name, including whatever the layers beneath contributed for it.
	value: Option<String>,
}

impl std::fmt::Debug for RequestBody {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::None => f.write_str("None"),
			Self::Bytes(bytes) => f.debug_tuple("Bytes").field(&bytes.len()).finish(),
			Self::Stream(_) => f.write_str("Stream"),
		}
	}
}

/// A request prepared but not sent.
///
/// Inert: it carries no agent, so it can be sent on more than one, and passing it to
/// [`Agent::fetch`] returns a builder that layers over it.
// spec:REQ
#[derive(Debug)]
pub struct Request {
	pub(super) url: Url,
	pub(super) options: RequestOptions,
	pub(super) body: RequestBody,
}

impl Request {
	/// Prepare a request aimed at `target`.
	pub fn new<T>(target: T) -> RequestBuilder
	where
		T: TryInto<Target>,
		T::Error: Into<FaithError>,
	{
		RequestBuilder {
			layer: Layer::over(target),
		}
	}

	pub fn url(&self) -> &Url {
		&self.url
	}

	/// Copy the request, when its body allows it.
	///
	/// `None` when the body is a stream, a stream being consumable once.
	// spec:REQ
	pub fn try_clone(&self) -> Option<Self> {
		let body = match &self.body {
			RequestBody::None => RequestBody::None,
			RequestBody::Bytes(bytes) => RequestBody::Bytes(bytes.clone()),
			RequestBody::Stream(_) => return None,
		};

		Some(Self {
			url: self.url.clone(),
			options: self.options.clone(),
			body,
		})
	}
}

/// One layer of settings over a target, which both builders are made of.
struct Layer {
	/// The first failure met while setting up, held until the builder resolves.
	// spec:REQ
	target: Result<Target, FaithError>,
	options: RequestOptions,
	/// Which single-valued settings this layer set explicitly, so an unset one is inherited rather
	/// than overwritten with a default.
	set: SetFlags,
	headers: Vec<HeaderOp>,
	body: Option<RequestBody>,
}

#[derive(Default)]
struct SetFlags {
	#[cfg(feature = "cache")]
	cache: bool,
	#[cfg(feature = "encoding")]
	compress: bool,
	credentials: bool,
	integrity: bool,
	method: bool,
	priority: bool,
	timeout: bool,
}

impl Layer {
	/// Hold a failure until the builder resolves, keeping the first one met.
	// spec:REQ
	fn fail(&mut self, err: FaithError) {
		if self.target.is_ok() {
			self.target = Err(err);
		}
	}

	fn over<T>(target: T) -> Self
	where
		T: TryInto<Target>,
		T::Error: Into<FaithError>,
	{
		Self {
			target: target.try_into().map_err(Into::into),
			options: RequestOptions::default(),
			set: SetFlags::default(),
			headers: Vec::new(),
			body: None,
		}
	}

	/// Settle this layer over whatever it wraps: the outermost explicit value wins, a setting this
	/// layer does not touch is inherited unchanged, and headers merge by name.
	// spec:REQ
	fn settle(self) -> Result<Request, FaithError> {
		let (url, mut options, body) = match self.target? {
			Target::Url(url) => (url, RequestOptions::default(), RequestBody::None),
			// The URL comes from the target at the bottom of the stack.
			Target::Request(inner) => (inner.url, inner.options, inner.body),
		};

		#[cfg(feature = "cache")]
		if self.set.cache {
			options.cache = self.options.cache;
		}
		#[cfg(feature = "encoding")]
		if self.set.compress {
			options.compress = self.options.compress;
		}
		if self.set.credentials {
			options.credentials = self.options.credentials;
		}
		if self.set.integrity {
			options.integrity = self.options.integrity;
		}
		if self.set.method {
			options.method = self.options.method;
		}
		if self.set.priority {
			options.priority = self.options.priority;
		}
		if self.set.timeout {
			options.timeout = self.options.timeout;
		}

		let mut headers = options.headers.take().unwrap_or_default();
		for op in self.headers {
			headers.retain(|(name, _)| !name.eq_ignore_ascii_case(&op.name));
			if let Some(value) = op.value {
				headers.push((op.name, value));
			}
		}
		options.headers = (!headers.is_empty()).then_some(headers);

		Ok(Request {
			url,
			options,
			body: self.body.unwrap_or(body),
		})
	}
}

/// The setters both builders carry, so a call reads the same way whichever it is written against.
macro_rules! layer_setters {
	($builder:ident) => {
		impl $builder {
			/// The request method. Defaults to `GET`.
			///
			/// Takes a `Method` or anything that converts into one; a value that does not is
			/// reported where the builder resolves.
			pub fn method<M>(mut self, method: M) -> Self
			where
				M: TryInto<Method>,
			{
				match method.try_into() {
					Ok(method) => {
						self.layer.options.method = Some(method.to_string());
						self.layer.set.method = true;
					}
					Err(_) => self.layer.fail(FaithErrorKind::InvalidMethod.into()),
				}
				self
			}

			/// Set a header, replacing whatever any layer beneath contributed for that name.
			///
			/// Takes `HeaderName` and `HeaderValue` or anything that converts into them.
			pub fn header<N, V>(mut self, name: N, value: V) -> Self
			where
				N: TryInto<HeaderName>,
				V: TryInto<HeaderValue>,
			{
				match (name.try_into(), value.try_into()) {
					(Ok(name), Ok(value)) => match value.to_str() {
						Ok(value) => self.layer.headers.push(HeaderOp {
							name: name.to_string(),
							value: Some(value.to_owned()),
						}),
						Err(_) => self.layer.fail(FaithErrorKind::InvalidHeader.into()),
					},
					_ => self.layer.fail(FaithErrorKind::InvalidHeader.into()),
				}
				self
			}

			/// Set several headers, each applied as a single header would be.
			pub fn headers<N, V>(mut self, headers: impl IntoIterator<Item = (N, V)>) -> Self
			where
				N: TryInto<HeaderName>,
				V: TryInto<HeaderValue>,
			{
				for (name, value) in headers {
					self = self.header(name, value);
				}
				self
			}

			/// Remove a header, including whatever the layers beneath contributed for it.
			pub fn remove_header<N>(mut self, name: N) -> Self
			where
				N: TryInto<HeaderName>,
			{
				match name.try_into() {
					Ok(name) => self.layer.headers.push(HeaderOp {
						name: name.to_string(),
						value: None,
					}),
					Err(_) => self.layer.fail(FaithErrorKind::InvalidHeader.into()),
				}
				self
			}

			/// A body already in hand.
			pub fn body(mut self, body: impl Into<Bytes>) -> Self {
				self.layer.body = Some(RequestBody::Bytes(body.into()));
				self
			}

			/// A body arriving in chunks, which goes out chunked having no length to declare.
			pub fn body_stream(
				mut self,
				body: impl Stream<Item = std::io::Result<Bytes>> + Send + 'static,
			) -> Self {
				self.layer.body = Some(RequestBody::Stream(Box::pin(body)));
				self
			}

			/// Bound the whole request and response.
			// spec:CANCEL
			pub fn timeout(mut self, timeout: Duration) -> Self {
				self.layer.options.timeout = Some(timeout);
				self.layer.set.timeout = true;
				self
			}

			/// The digests the response body is expected to match.
			// spec:SRI
			pub fn integrity(mut self, integrity: impl Into<String>) -> Self {
				self.layer.options.integrity = Some(integrity.into());
				self.layer.set.integrity = true;
				self
			}

			/// Compress the request body in this coding, named by its wire token.
			// spec:ENC
			#[cfg(feature = "encoding")]
			pub fn compress(mut self, coding: impl Into<String>) -> Self {
				self.layer.options.compress = Some(coding.into());
				self.layer.set.compress = true;
				self
			}

			/// How the HTTP cache is consulted for this request.
			// spec:CACHE
			#[cfg(feature = "cache")]
			pub fn cache(mut self, mode: CacheMode) -> Self {
				self.layer.options.cache = mode;
				self.layer.set.cache = true;
				self
			}

			/// Whether the request carries its credentials.
			pub fn credentials(mut self, credentials: Credentials) -> Self {
				self.layer.options.credentials = credentials;
				self.layer.set.credentials = true;
				self
			}

			/// How this request ranks against others, as an RFC 9218 urgency.
			// spec:REQ#request-priority
			pub fn priority(mut self, priority: Priority) -> Self {
				self.layer.options.priority = priority.urgency();
				self.layer.set.priority = true;
				self
			}
		}
	};
}

/// How a request ranks against others.
// spec:REQ#request-priority
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Priority {
	/// More urgent than an unmarked request.
	High,
	/// Less urgent than an unmarked request.
	Low,
	/// Send no `Priority` header, leaving the request at the scheme's default urgency.
	#[default]
	Auto,
}

impl Priority {
	fn urgency(self) -> Option<&'static str> {
		match self {
			Self::High => Some("u=1"),
			Self::Low => Some("u=5"),
			Self::Auto => None,
		}
	}
}

/// Prepares a [`Request`] without sending it.
#[must_use = "a request builder does nothing until built"]
pub struct RequestBuilder {
	layer: Layer,
}

layer_setters!(RequestBuilder);

impl RequestBuilder {
	/// Settle the layers into a request.
	///
	/// Where a conversion failed on the way in — an unparseable target, say — it is reported here.
	pub fn build(self) -> Result<Request, FaithError> {
		self.layer.settle()
	}
}

/// A request bound to an agent, which sends when awaited.
///
/// There is no separate send step: awaiting is what sends. A builder dropped without being awaited
/// sends nothing, and dropping the future mid-flight cancels the request.
// spec:REQ spec:CANCEL
#[must_use = "a fetch builder sends nothing until awaited"]
pub struct FetchBuilder {
	agent: Agent,
	layer: Layer,
}

layer_setters!(FetchBuilder);

impl FetchBuilder {
	/// Settle the layers into the request that would be sent, without sending it.
	pub fn build(self) -> Result<Request, FaithError> {
		self.layer.settle()
	}
}

impl IntoFuture for FetchBuilder {
	type Output = Result<Response, FaithError>;
	type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

	fn into_future(self) -> Self::IntoFuture {
		Box::pin(async move {
			// Taken before anything is awaited, so the request counts as in flight from here.
			// spec:AGENT
			let client = self.agent.client().ok_or(FaithErrorKind::Closed)?;
			let request = self.layer.settle()?;

			send(
				&self.agent,
				client,
				request.url.as_str(),
				request.options,
				request.body,
				None::<std::future::Pending<()>>,
			)
			.await
		})
	}
}

impl Agent {
	/// Aim a request at `target`, to send when awaited.
	///
	/// The target is a URL, or a [`Request`] to layer over. Awaiting the builder sends the request;
	/// see [`FetchBuilder`].
	// spec:REQ
	pub fn fetch<T>(&self, target: T) -> FetchBuilder
	where
		T: TryInto<Target>,
		T::Error: Into<FaithError>,
	{
		FetchBuilder {
			agent: self.clone(),
			layer: Layer::over(target),
		}
	}
}

#[cfg(test)]
#[cfg(test)]
mod tests;
