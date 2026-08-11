use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
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
	header::{ACCEPT_ENCODING, HeaderName, HeaderValue},
	tls::TlsInfo,
};
use tokio::sync::{Mutex, mpsc};

use crate::{
	async_task::faith_promise,
	body::{Body, BodyHolder},
	encoding::{self, AcceptEncoding, DEFAULT_ACCEPT_ENCODING},
	error::{FaithError, FaithErrorKind},
	options::{CredentialsOption, FaithOptions, FaithOptionsAndBody},
	response::{FaithResponse, PeerInformation},
	stream_body::StreamBody,
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

		let mut request = agent
			.client
			.as_ref()
			.ok_or(FaithErrorKind::Closed)?
			.request(method, parsed_url.clone())
			.with_extension(CacheMode::from(options.cache));

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

		// The request's `Accept-Encoding` governs which codings Fáith decodes on the way
		// back (spec: ENC): a value on the request, else one inherited from the agent's
		// default headers, else the default Fáith sends itself. Neither the request nor the
		// agent advertising a value means nothing beneath Fáith adds one now that it owns
		// the codings, so Fáith sends the default explicitly.
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

		// Handle body: prefer streaming body over buffered body
		if let Some(receiver_arc) = stream_receiver {
			// Take the receiver from the Arc<Mutex<Option<...>>>
			let receiver = {
				let mut guard = receiver_arc.lock().await;
				guard.take()
			};

			if let Some(receiver) = receiver {
				// Convert the receiver into a stream for reqwest
				let byte_stream = receiver.into_stream();
				request = request.body(reqwest::Body::wrap_stream(byte_stream));
			}
		} else if let Some(body) = &body {
			request = request.body(body.to_vec());
		}

		if let Some(dur) = options.timeout {
			request = request.timeout(dur);
		}

		agent.stats.requests_sent.fetch_add(1, Ordering::Relaxed);

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

		// Track connection for TCP stats (if we can get both local and remote addr)
		if let Some(http_info) = response.extensions().get::<HttpInfo>() {
			let local_addr = http_info.local_addr();
			let remote_addr = http_info.remote_addr();
			agent.conn_tracker.track(local_addr, remote_addr);
		}

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

		// Decode only a body Fáith negotiated the coding for; a bodyless response keeps its
		// `Content-Encoding` and `Content-Length` describing the representation (spec: ENC).
		let decode = if empty {
			None
		} else {
			encoding::decision(&headers, &accept_encoding)
		};
		if decode.is_some() {
			encoding::strip_decoded_headers(&mut headers);
		}

		Ok(FaithResponse {
			body: if empty {
				BodyHolder::none()
			} else {
				let http_response: http::Response<_> = response.into();
				BodyHolder::new(
					Some(Arc::new(Mutex::new(Body::Inner(http_response.into_body())))),
					version,
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
			trailers: Default::default(),
			url: response_url,
			version,
		})
	})
}
