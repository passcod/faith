use std::{
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::Instant,
};

use http_cache_reqwest::CacheMode;
use hyper_util::client::legacy::connect::HttpInfo;
use napi::{
	Env,
	bindgen_prelude::{AbortSignal, PromiseRaw},
};
use napi_derive::napi;
use reqwest::{Method, StatusCode};
use reqwest::{
	header::{ACCEPT_ENCODING, CONTENT_ENCODING, HeaderName, HeaderValue},
	tls::TlsInfo,
};
use tokio::sync::{Mutex, mpsc};

use crate::{
	async_task::faith_promise,
	body::{Body, BodyHolder},
	encoding::{self, AcceptEncoding, DEFAULT_ACCEPT_ENCODING},
	error::{FaithError, FaithErrorKind},
	options::{CredentialsOption, DuplexOption, FaithOptions, FaithOptionsAndBody, PRIORITY},
	response::{FaithResponse, PeerInformation},
	stream_body::StreamBody,
	timing::{HeadersStamp, RequestTiming, TimingSlot, alpn_protocol_id},
};

/// The methods the fetch standard normalises to upper case; any other method is sent as given.
const NORMALISED_METHODS: [&str; 6] = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"];

#[napi]
pub fn faith_fetch<'env>(
	env: &'env Env,
	url: String,
	options: FaithOptionsAndBody,
	signal: Option<AbortSignal>,
	stream_body: Option<&StreamBody>,
) -> Result<PromiseRaw<'env, FaithResponse>, napi::Error> {
	let (options, agent, body) = FaithOptions::extract(options);
	let (s, abort) = mpsc::channel(8);
	let has_signal = signal.is_some();
	if let Some(signal) = signal {
		signal.on_abort(move || {
			let _ = s.try_send(());
		});
	}

	// Get the stream body receiver if provided
	let stream_receiver = stream_body.map(|sb| sb.receiver.clone());

	faith_promise(env, async move {
		let mut abort = abort;
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

		// Handle credentials based on credentials option
		if options.credentials == CredentialsOption::Omit {
			// Remove credentials from URL if omit is specified
			let _ = parsed_url.set_username("");
			let _ = parsed_url.set_password(None);
		}

		// The stamp rides along in the request's extensions for the middleware to fill in;
		// this side keeps a handle on it so the one measurement taken inside the stack is
		// the one surfaced (spec:RESP#request-timing).
		let headers_stamp = HeadersStamp::default();

		let mut request = agent
			.client
			.as_ref()
			.ok_or(FaithErrorKind::Closed)?
			.request(method, parsed_url.clone())
			.with_extension(CacheMode::from(options.cache))
			.with_extension(headers_stamp.clone());

		if let Some(headers) = &options.headers {
			for (key, value) in headers {
				// Skip Cookie header if credentials is omit
				if options.credentials == CredentialsOption::Omit
					&& key.eq_ignore_ascii_case("cookie")
				{
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
				request = request.header(header_name, header_value);
			}
		}

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

		// Settles once a streaming request body has been written in full; `None` when the
		// request carries no streaming body, which has nothing to wait for.
		let mut body_sent = None;

		// Handle body: prefer streaming body over buffered body
		if let Some(receiver_arc) = stream_receiver {
			// Take the receiver from the Arc<Mutex<Option<...>>>
			let receiver = {
				let mut guard = receiver_arc.lock().await;
				guard.take()
			};

			if let Some(receiver) = receiver {
				// Convert the receiver into a stream for reqwest
				let (byte_stream, sent) = receiver.into_stream();
				body_sent = Some(sent);
				request = request.body(reqwest::Body::wrap_stream(byte_stream));
			}
		} else if let Some(body) = &body {
			request = request.body(body.to_vec());
		}

		if let Some(dur) = options.timeout {
			request = request.timeout(dur);
		}

		agent.stats.requests_sent.fetch_add(1, Ordering::Relaxed);

		// The origin every phase is measured from.
		let started = Instant::now();

		// Race the request with the abort signal if signal was provided
		let response = if has_signal {
			tokio::select! {
				result = request.send() => result?,
				_ = abort.recv() => {
					return Err(FaithErrorKind::Aborted.into());
				}
			}
		} else {
			request.send().await?
		};

		// A half-duplex request does not surface its response until the whole request has gone
		// out, even where the origin answered sooner. The transport is duplex either way; this
		// is the sequencing the caller asked for.
		if options.duplex != Some(DuplexOption::Full)
			&& let Some(sent) = body_sent.take()
		{
			if has_signal {
				tokio::select! {
					_ = sent => {}
					_ = abort.recv() => {
						return Err(FaithErrorKind::Aborted.into());
					}
				}
			} else {
				let _ = sent.await;
			}
		}

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
		if options.credentials == CredentialsOption::Omit {
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

		Ok(FaithResponse {
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
	})
}
