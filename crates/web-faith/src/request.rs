//! Sending a request, and building the response that comes back.

// spec:REQ spec:ENC spec:CANCEL

use std::{
	future::{Future, IntoFuture},
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, Instant},
};

use bytes::Bytes;
use futures::Stream;
use http_cache_reqwest::CacheMode;
use hyper_util::client::legacy::connect::HttpInfo;
use reqwest::{
	Method, StatusCode,
	header::{ACCEPT_ENCODING, CONTENT_ENCODING, HeaderName, HeaderValue},
	tls::TlsInfo,
};
use reqwest_middleware::ClientWithMiddleware;
use tokio::sync::Mutex;
use url::Url;
use web_faith_encoding::{self as encoding, AcceptEncoding, Coding, DEFAULT_ACCEPT_ENCODING};

use crate::{
	agent::Agent,
	body::{Body, BodyHolder},
	error::{FaithError, FaithErrorKind},
	response::{PeerInformation, Response},
	timing::{HeadersStamp, RequestTiming, TimingSlot, alpn_protocol_id},
};

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
	pub cache: CacheMode,
	/// A coding to compress the body in, named by its wire token.
	pub compress: Option<String>,
	pub credentials: Credentials,
	pub headers: Option<Vec<(String, String)>>,
	pub integrity: Option<String>,
	pub method: Option<String>,
	/// The `Priority` header value this request's priority derives, if it derives one.
	pub priority: Option<&'static str>,
	pub timeout: Option<Duration>,
}

/// Send a request on `agent`, and build the response it produces.
///
/// `client` is the handle the caller took when the request was issued, rather than one taken here:
/// a request counts as in flight from the moment it is issued, so one issued just before the agent
/// closes runs to completion even though nothing had started on it yet.
///
/// `abort` is an optional future that, resolving first, cancels the request.
// spec:AGENT
pub async fn send(
	agent: &Agent,
	client: ClientWithMiddleware,
	url: &str,
	options: RequestOptions,
	body: RequestBody,
	abort: Option<impl Future<Output = ()>>,
) -> Result<Response, FaithError> {
	let method = options.method.as_deref().unwrap_or("GET");
	// spec:REQ#method-and-headers
	let method = NORMALISED_METHODS
		.into_iter()
		.find(|normalised| normalised.eq_ignore_ascii_case(method))
		.unwrap_or(method);

	let method =
		Method::from_bytes(method.as_bytes()).map_err(|_| FaithErrorKind::InvalidMethod)?;
	let is_head = method == Method::HEAD;

	let mut parsed_url = reqwest::Url::parse(&url).map_err(|_| FaithErrorKind::InvalidUrl)?;

	// A `compress` naming no coding Faith can compress in is misuse whether or not the
	// request turns out to carry a body, so it is refused before anything else looks at
	// it (spec:ENC#compressing-a-request-body).
	let compress = options
		.compress
		.as_deref()
		.map(|value| {
			Coding::from_option(value).ok_or_else(|| {
				FaithError::new(
					FaithErrorKind::InvalidCompression,
					Some(format!(
						"compress: {value:?} names no coding; expected gzip, deflate, br, or zstd"
					)),
				)
			})
		})
		.transpose()?;

	// Handle credentials based on credentials option
	if options.credentials == Credentials::Omit {
		// Remove credentials from URL if omit is specified
		let _ = parsed_url.set_username("");
		let _ = parsed_url.set_password(None);
	}

	// The stamp rides along in the request's extensions for the middleware to fill in;
	// this side keeps a handle on it so the one measurement taken inside the stack is
	// the one surfaced (spec:RESP#request-timing).
	let headers_stamp = HeadersStamp::default();

	let mut request = client
		.request(method, parsed_url.clone())
		.with_extension(CacheMode::from(options.cache))
		.with_extension(headers_stamp.clone());

	if let Some(headers) = &options.headers {
		for (key, value) in headers {
			// Skip Cookie header if credentials is omit
			if options.credentials == Credentials::Omit && key.eq_ignore_ascii_case("cookie") {
				continue;
			}

			// Validate header name and value before adding to request
			let header_name = HeaderName::from_bytes(key.as_bytes()).map_err(|_| {
				FaithError::new(
					FaithErrorKind::InvalidHeader,
					Some(format!("invalid header name: {key}")),
				)
			})?;
			let header_value = HeaderValue::from_str(value).map_err(|_| {
				FaithError::new(
					FaithErrorKind::InvalidHeader,
					Some(format!("invalid header value: {value}")),
				)
			})?;

			// Faith's coding is layered on top of what the caller declares, and reqwest's
			// builder appends rather than replaces, so passing the caller's value through
			// here would put a second `Content-Encoding` beside the joined one -- the same
			// list read twice over (spec:ENC#what-a-compressed-request-sends). The value is
			// still validated above, then withheld and re-emitted once below.
			if compress.is_some() && header_name == CONTENT_ENCODING {
				continue;
			}

			request = request.header(header_name, header_value);
		}
	}

	// What the caller says they handed over: their own `Content-Encoding`, else the
	// agent's, per-request headers winning per name as they do generally (spec: REQ).
	// Several lines are the one list, so they are joined as they are read.
	let declared_content_encoding = compress.and_then(|_| {
		let from_request = options.headers.as_ref().and_then(|headers| {
			let declared = headers
				.iter()
				.filter(|(name, _)| name.eq_ignore_ascii_case(CONTENT_ENCODING.as_str()))
				.map(|(_, value)| value.as_str())
				.collect::<Vec<_>>();
			(!declared.is_empty()).then(|| declared.join(", "))
		});
		from_request.or_else(|| {
			agent
				.default_content_encoding
				.as_ref()
				.and_then(|value| value.to_str().ok().map(str::to_owned))
		})
	});

	// The request's `Accept-Encoding` governs which codings Faith decodes on the way
	// back (spec: ENC): a value on the request, else one inherited from the agent's
	// default headers, else the default Faith sends itself. Neither the request nor the
	// agent advertising a value means nothing beneath Faith adds one now that it owns
	// the codings, so Faith sends the default explicitly.
	let request_accept_encoding = options.headers.as_ref().and_then(|headers| {
		headers
			.iter()
			.find(|(name, _)| name.eq_ignore_ascii_case("accept-encoding"))
			.map(|(_, value)| value.clone())
	});
	let accept_encoding = AcceptEncoding::parse(
		&request_accept_encoding
			.clone()
			.or_else(|| {
				agent
					.default_accept_encoding
					.as_ref()
					.and_then(|value| value.to_str().ok().map(str::to_owned))
			})
			.unwrap_or_else(|| DEFAULT_ACCEPT_ENCODING.to_owned()),
	);
	if request_accept_encoding.is_none() && agent.default_accept_encoding.is_none() {
		request = request.header(
			ACCEPT_ENCODING,
			HeaderValue::from_static(DEFAULT_ACCEPT_ENCODING),
		);
	}

	// The `priority` option is a hint, so a `Priority` header the caller wrote, or one
	// among the agent's default headers, wins over the value derived from it
	// (spec: REQ#request-priority). The agent's defaults are consulted here rather than
	// left to reqwest: it fills a default header in only where the request carries none
	// of that name, so setting the derived value would displace the agent's own.
	if let Some(urgency) = options.priority
		&& !agent.has_default_priority
		&& !options.headers.as_ref().is_some_and(|headers| {
			headers
				.iter()
				.any(|(name, _)| name.eq_ignore_ascii_case(PRIORITY))
		}) {
		request = request.header(
			HeaderName::from_static(PRIORITY),
			HeaderValue::from_static(urgency),
		);
	}

	// The coding actually applied, which is `compress` only where there was a body to
	// apply it to: the option does nothing on a request carrying none, so no
	// `Content-Encoding` describes bytes that were never sent
	// (spec:ENC#compressing-a-request-body).
	let mut applied_coding = None;

	// Handle body: prefer streaming body over buffered body
	match body {
		RequestBody::Stream(byte_stream) => {
			// A body read from a stream has no length to advertise, which the fetch standard
			// allows only over HTTP/2 and HTTP/3.
			// spec:REQ#streaming-a-request-body
			if !agent.quirk_h1_request_streaming {
				// Faith never negotiates h2c, so a plaintext origin is HTTP/1.x for certain and
				// can be refused without opening a connection to find out. Returning here drops
				// the stream, which is what tells whatever is feeding it to stop.
				if parsed_url.scheme() != "https" {
					return Err(FaithError::new(
						FaithErrorKind::Network,
						Some(format!(
							"a streaming request body requires HTTP/2 or HTTP/3, and {} is served over HTTP/1.1; set the agent's quirks.h1RequestStreaming to send it anyway",
							parsed_url.as_str()
						)),
					));
				}

				// Over TLS the protocol is only known once ALPN has run. Asserting HTTP/2 on the
				// request hands the check to the layer that finds out: the connection is chosen,
				// and an HTTP/1.x one is refused there before any of the body is written.
				request = request.version(http::Version::HTTP_2);
			}

			request = request.body(match compress {
				// Compressed as the chunks arrive, and chunked on the wire either way:
				// a stream has no length to declare up front.
				// spec:ENC#what-a-compressed-request-sends
				Some(coding) => {
					applied_coding = Some(coding);
					reqwest::Body::wrap_stream(encoding::compress_stream(byte_stream, coding))
				}
				None => reqwest::Body::wrap_stream(byte_stream),
			});
		}
		RequestBody::Bytes(bytes) => {
			request = request.body(match compress {
				// The compressed bytes are what reqwest sizes `Content-Length` from, so the
				// header counts what goes on the wire.
				// spec:ENC#what-a-compressed-request-sends
				Some(coding) => {
					applied_coding = Some(coding);
					encoding::compress_buffer(&bytes, coding)
						.await
						.map_err(|err| {
							FaithError::new(
								FaithErrorKind::Network,
								Some(format!("could not compress the request body: {err}")),
							)
						})?
				}
				None => bytes.to_vec(),
			});
		}
		RequestBody::None => {}
	}

	// One `Content-Encoding` naming the caller's codings then Faith's, in the order they
	// were applied (spec:ENC#what-a-compressed-request-sends).
	if let Some(coding) = applied_coding {
		let value = encoding::layer_content_encoding(declared_content_encoding.as_deref(), coding);
		let value = HeaderValue::from_str(&value).map_err(|_| {
			FaithError::new(
				FaithErrorKind::InvalidHeader,
				Some(format!("invalid header value: {value}")),
			)
		})?;
		request = request.header(CONTENT_ENCODING, value);
	}

	if let Some(dur) = options.timeout {
		request = request.timeout(dur);
	}

	agent.stats.requests_sent.fetch_add(1, Ordering::Relaxed);

	// The origin every phase is measured from.
	let started = Instant::now();

	// A caller that can abort races the request against that signal; one that cannot just sends.
	let response = match abort {
		Some(abort) => {
			tokio::select! {
				result = request.send() => result?,
				_ = abort => {
					return Err(FaithErrorKind::Aborted.into());
				}
			}
		}
		None => request.send().await?,
	};

	agent
		.stats
		.responses_received
		.fetch_add(1, Ordering::Relaxed);

	let status_code = response.status();
	let empty = status_code == StatusCode::NO_CONTENT || is_head;

	let response_url = response.url().clone();
	let version = response.version();

	// With `http3.upgradeFollowAdvertisedPort` on, an HTTP/3 attempt rewrites the
	// request's port to the advertised one, so the response URL's port reflects
	// which endpoint answered rather than any redirect. Compare with ports
	// normalised away, or every such request would report `redirected`.
	//
	// Only HTTP/3 responses can have been rewritten — reqwest routes
	// `Version::HTTP_3` exclusively to the h3 client with no silent downgrade, and
	// the TCP fallback re-runs the untouched clone. Restricting the normalisation
	// to those keeps exact comparison, and so port-only redirect detection, for
	// every other response.
	let redirected = if agent.h3_follow_advertised_port && version == http::Version::HTTP_3 {
		let without_port = |url: &reqwest::Url| {
			let mut url = url.clone();
			let _ = url.set_port(None);
			url
		};
		without_port(&parsed_url) != without_port(&response_url)
	} else {
		parsed_url != response_url
	};

	// Track connection for TCP stats (if we can get both local and remote addr).
	// A connection the tracker has already seen is one the pool handed back, which is
	// what `reused` reports (spec:RESP#request-timing).
	let reused = if let Some(http_info) = response.extensions().get::<HttpInfo>() {
		let local_addr = http_info.local_addr();
		let remote_addr = http_info.remote_addr();
		agent.conn_tracker.track(local_addr, remote_addr)
	} else {
		false
	};

	// The origin now holds a connection the pool keeps idle, so a `preconnect` for it has
	// nothing left to do (spec:WARM). Keyed on the URL the request was sent to, so a
	// redirect chain marks the origin that actually answered rather than the one asked for.
	agent.mark_warm(&response_url);

	let peer = PeerInformation {
		address: response.remote_addr(),
		certificate: response
			.extensions()
			.get::<TlsInfo>()
			.and_then(|info| info.peer_certificate())
			.map(|cert| cert.into()),
	};

	let mut headers = response.headers().clone();
	if options.credentials == Credentials::Omit {
		headers.remove("set-cookie");
	}

	// A cache hit is served without ever reaching the layer that stamps, so fall back to
	// the moment the send resolved, which for a hit is the moment the cache answered.
	let headers_at = headers_stamp.get().unwrap_or_else(Instant::now);
	let timing = RequestTiming {
		headers_ms: headers_at.duration_since(started).as_secs_f64() * 1000.0,
		body_ms: None,
		reused,
		next_hop_protocol: alpn_protocol_id(version, &response_url),
		// Captured before a decoded body's `Content-Encoding` is stripped below, so the
		// coding the response arrived under is reported either way.
		content_encoding: headers
			.get(CONTENT_ENCODING)
			.and_then(|value| value.to_str().ok())
			.map(str::to_owned),
		from_cache: headers
			.get("x-cache")
			.and_then(|value| value.to_str().ok())
			.is_some_and(|value| value.eq_ignore_ascii_case("HIT")),
	};

	// Decode only a body Faith negotiated the coding for; a bodyless response keeps its
	// `Content-Encoding` and `Content-Length` describing the representation (spec: ENC).
	let decode = if empty {
		None
	} else {
		encoding::decision(&headers, &accept_encoding)
	};
	if decode.is_some() {
		encoding::strip_decoded_headers(&mut headers);
	}

	let timing = Arc::new(TimingSlot::new(started, timing));
	// A response that cannot carry a body has nothing left to wait for.
	if empty {
		timing.ended();
	}

	Ok(Response {
		body: if empty {
			BodyHolder::none()
		} else {
			let http_response: http::Response<_> = response.into();
			BodyHolder::new(
				Some(Arc::new(Mutex::new(Body::Inner(http_response.into_body())))),
				version,
				timing.clone(),
			)
		},
		decode,
		disturbed: Arc::new(AtomicBool::new(false)),
		headers,
		integrity: options.integrity,
		peer: Arc::new(peer),
		redirected,
		stats: agent.stats.clone(),
		status_code,
		timing,
		trailers: Default::default(),
		url: response_url,
		version,
	})
}

/// What a request is aimed at: a URL, or another request to layer over.
///
/// Anything that converts into a `url::Url` is a target, as is a [`Request`], which is what lets a
/// prepared request be adjusted at each call site.
// spec:REQ
pub enum Target {
	Url(Url),
	Request(Box<Request>),
}

impl From<Url> for Target {
	fn from(url: Url) -> Self {
		Self::Url(url)
	}
}

impl From<Request> for Target {
	fn from(request: Request) -> Self {
		Self::Request(Box::new(request))
	}
}

/// A string target is parsed when the builder resolves, so an unparseable one is reported there
/// rather than by a call that cannot fail.
// spec:REQ
impl TryFrom<&str> for Target {
	type Error = FaithError;

	fn try_from(url: &str) -> Result<Self, Self::Error> {
		Url::parse(url)
			.map(Self::Url)
			.map_err(|_| FaithErrorKind::InvalidUrl.into())
	}
}

impl TryFrom<String> for Target {
	type Error = FaithError;

	fn try_from(url: String) -> Result<Self, Self::Error> {
		Self::try_from(url.as_str())
	}
}

/// An `http::Request` is a target too, so a request built against the wider ecosystem can be sent
/// on an agent: its method, URL, headers, and body come across.
// spec:REQ
impl<B> TryFrom<http::Request<B>> for Target
where
	B: Into<Bytes>,
{
	type Error = FaithError;

	fn try_from(request: http::Request<B>) -> Result<Self, Self::Error> {
		let (parts, body) = request.into_parts();

		let url = Url::parse(&parts.uri.to_string())
			.map_err(|_| FaithError::from(FaithErrorKind::InvalidUrl))?;

		let mut headers = Vec::new();
		for (name, value) in parts.headers.iter() {
			let Ok(value) = value.to_str() else {
				return Err(FaithErrorKind::InvalidHeader.into());
			};
			headers.push((name.to_string(), value.to_owned()));
		}

		let body = body.into();
		Ok(Self::Request(Box::new(Request {
			url,
			options: RequestOptions {
				method: Some(parts.method.to_string()),
				headers: (!headers.is_empty()).then_some(headers),
				..RequestOptions::default()
			},
			body: if body.is_empty() {
				RequestBody::None
			} else {
				RequestBody::Bytes(body)
			},
		})))
	}
}

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
	url: Url,
	options: RequestOptions,
	body: RequestBody,
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

	pub fn options(&self) -> &RequestOptions {
		&self.options
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
	cache: bool,
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

		if self.set.cache {
			options.cache = self.options.cache;
		}
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
			pub fn compress(mut self, coding: impl Into<String>) -> Self {
				self.layer.options.compress = Some(coding.into());
				self.layer.set.compress = true;
				self
			}

			/// How the HTTP cache is consulted for this request.
			// spec:CACHE
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
mod tests {
	use super::*;

	fn headers_of(request: &Request) -> Vec<(String, String)> {
		request.options.headers.clone().unwrap_or_default()
	}

	/// The outermost layer that set a value wins, whatever the layers beneath said.
	#[test]
	fn the_outermost_explicit_value_wins() {
		let inner = Request::new("https://example.com/")
			.timeout(Duration::from_secs(1))
			.build()
			.expect("a valid target");

		let outer = Request::new(inner)
			.timeout(Duration::from_secs(3))
			.build()
			.expect("layering over a request");

		assert_eq!(outer.options.timeout, Some(Duration::from_secs(3)));
	}

	/// A setting an outer layer does not touch is inherited unchanged.
	#[test]
	fn an_untouched_setting_is_inherited() {
		let inner = Request::new("https://example.com/")
			.timeout(Duration::from_secs(1))
			.method("POST")
			.build()
			.expect("a valid target");

		let outer = Request::new(inner)
			.integrity("sha256-abc")
			.build()
			.expect("layering over a request");

		assert_eq!(outer.options.timeout, Some(Duration::from_secs(1)));
		assert_eq!(outer.options.method.as_deref(), Some("POST"));
		assert_eq!(outer.options.integrity.as_deref(), Some("sha256-abc"));
	}

	/// The URL comes from the target at the bottom of the stack.
	#[test]
	fn wrapping_a_request_carries_its_url_through() {
		let inner = Request::new("https://example.com/deep/path?q=1")
			.build()
			.expect("a valid target");
		let outer = Request::new(inner)
			.method("HEAD")
			.build()
			.expect("layering");

		assert_eq!(outer.url.as_str(), "https://example.com/deep/path?q=1");
	}

	/// Headers merge by name: an outer value replaces, a name only set inside carries through.
	#[test]
	fn headers_merge_by_name() {
		let inner = Request::new("https://example.com/")
			.header("x-keep", "inner")
			.header("x-replace", "inner")
			.build()
			.expect("a valid target");

		let outer = Request::new(inner)
			.header("x-replace", "outer")
			.header("x-add", "outer")
			.build()
			.expect("layering over a request");

		let mut headers = headers_of(&outer);
		headers.sort();
		assert_eq!(
			headers,
			vec![
				("x-add".to_owned(), "outer".to_owned()),
				("x-keep".to_owned(), "inner".to_owned()),
				("x-replace".to_owned(), "outer".to_owned()),
			]
		);
	}

	/// Removing a name removes what the layers beneath contributed for it.
	#[test]
	fn removing_a_header_removes_what_is_underneath() {
		let inner = Request::new("https://example.com/")
			.header("x-gone", "inner")
			.build()
			.expect("a valid target");

		let outer = Request::new(inner)
			.remove_header("X-Gone")
			.build()
			.expect("layering over a request");

		assert!(headers_of(&outer).is_empty(), "the name was removed");
	}

	/// Setting the same thing twice on one builder is the same question at a smaller scale.
	#[test]
	fn the_later_call_wins_on_one_builder() {
		let request = Request::new("https://example.com/")
			.method("POST")
			.method("PUT")
			.build()
			.expect("a valid target");

		assert_eq!(request.options.method.as_deref(), Some("PUT"));
	}

	/// An `http::Request` brings its method, URL, headers, and body across.
	#[test]
	fn an_http_request_is_a_target() {
		let http = http::Request::builder()
			.method(http::Method::POST)
			.uri("https://example.com/submit")
			.header("x-from", "ecosystem")
			.body(Bytes::from_static(b"payload"))
			.expect("a valid http request");

		let request = Request::new(http).build().expect("it converts");

		assert_eq!(request.url.as_str(), "https://example.com/submit");
		assert_eq!(request.options.method.as_deref(), Some("POST"));
		assert_eq!(
			headers_of(&request),
			vec![("x-from".to_owned(), "ecosystem".to_owned())]
		);
		assert!(request.try_clone().is_some(), "a buffered body copies");
	}

	/// A layer over an `http::Request` beats what it carried, as over any other target.
	#[test]
	fn a_layer_beats_what_an_http_request_carried() {
		let http = http::Request::builder()
			.method(http::Method::POST)
			.uri("https://example.com/")
			.body(Bytes::new())
			.expect("a valid http request");

		let request = Request::new(http)
			.method(http::Method::PUT)
			.build()
			.expect("layering over it");

		assert_eq!(request.options.method.as_deref(), Some("PUT"));
	}

	/// A header name that is not one is reported where the request is resolved too.
	#[test]
	fn an_invalid_header_surfaces_at_build() {
		let err = Request::new("https://example.com/")
			.header("not a header name", "value")
			.build()
			.expect_err("the name is not a header name");

		assert_eq!(err.kind, FaithErrorKind::InvalidHeader);
	}

	/// The first failure met is the one reported, not the last.
	#[test]
	fn the_first_failure_is_the_one_reported() {
		let err = Request::new("not a url")
			.header("not a header name", "value")
			.build()
			.expect_err("both are wrong");

		assert_eq!(err.kind, FaithErrorKind::InvalidUrl);
	}

	/// An unparseable target is reported where the request is resolved, not by the call that took it.
	#[test]
	fn an_unparseable_target_surfaces_at_build() {
		let builder = Request::new("not a url");
		let err = builder.build().expect_err("the target does not parse");

		assert_eq!(err.kind, FaithErrorKind::InvalidUrl);
	}

	/// A request copies when its body allows it, and reports that it cannot when it does not.
	#[test]
	fn try_clone_copies_what_it_can() {
		let buffered = Request::new("https://example.com/")
			.body(Bytes::from_static(b"body"))
			.build()
			.expect("a valid target");
		assert!(buffered.try_clone().is_some());

		let streamed = Request::new("https://example.com/")
			.body_stream(futures::stream::empty())
			.build()
			.expect("a valid target");
		assert!(
			streamed.try_clone().is_none(),
			"a stream is consumable once"
		);
	}
}
