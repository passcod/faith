use std::sync::{
	Arc,
	atomic::{AtomicBool, Ordering},
};

use http_cache_reqwest::CacheMode;
use hyper_util::client::legacy::connect::HttpInfo;
use napi::bindgen_prelude::AbortSignal;
use napi_derive::napi;
use reqwest::{Method, StatusCode};
use reqwest::{
	header::{HeaderName, HeaderValue},
	tls::TlsInfo,
};
use tokio::sync::{Mutex, mpsc};

use crate::{
	async_task::{Async, FaithAsyncResult},
	body::{Body, BodyHolder},
	error::{FaithError, FaithErrorKind},
	options::{CredentialsOption, FaithOptions, FaithOptionsAndBody},
	response::{FaithResponse, PeerInformation},
	stream_body::StreamBody,
};

#[napi]
pub fn faith_fetch(
	url: String,
	options: FaithOptionsAndBody,
	signal: Option<AbortSignal>,
	stream_body: Option<&StreamBody>,
) -> Async<FaithResponse> {
	let (options, agent, body) = FaithOptions::extract(options);
	let (s, abort) = mpsc::channel(8);
	if let Some(signal) = &signal {
		signal.on_abort(move || {
			let _ = s.try_send(());
		});
	}
	let has_signal = signal.is_some();

	// Get the stream body receiver if provided
	let stream_receiver = stream_body.map(|sb| sb.receiver.clone());

	FaithAsyncResult::with_signal(signal, async move || {
		let mut abort = abort;
		let method = options
			.method
			.map(|m| m.to_uppercase())
			.unwrap_or_else(|| "GET".to_string());

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
		let redirected = parsed_url != response_url;

		let version = response.version();

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
