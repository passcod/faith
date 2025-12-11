use std::{
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use futures::StreamExt;
use napi::bindgen_prelude::AbortSignal;
use napi_derive::napi;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};
use stream_shared::SharedStream;
use tokio::sync::mpsc;

use crate::{
	async_task::{Async, FaithAsyncResult},
	body::{Body, DynStream},
	error::{FaithError, FaithErrorKind},
	options::{CredentialsOption, FaithOptions, FaithOptionsAndBody},
	response::FaithResponse,
};

#[napi]
pub fn faith_fetch(
	url: String,
	options: FaithOptionsAndBody,
	signal: Option<AbortSignal>,
) -> Async<FaithResponse> {
	let (options, agent, body) = FaithOptions::extract(options);
	let (s, abort) = mpsc::channel(8);
	if let Some(signal) = &signal {
		signal.on_abort(move || {
			let _ = s.try_send(());
		});
	}
	let has_signal = signal.is_some();
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

		let mut request = agent.client.request(method, parsed_url);

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

		if let Some(body) = &body {
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

		let status = response.status().as_u16();
		let status_text = response
			.status()
			.canonical_reason()
			.unwrap_or_default()
			.to_string();
		let ok = response.status().is_success();
		let url = response.url().to_string();
		let redirected = response.status().is_redirection();
		let version = format!("{:?}", response.version());

		let headers_vec: Vec<(String, String)> = response
			.headers()
			.iter()
			.filter_map(|(name, value)| {
				// Skip Set-Cookie header if credentials is omit
				if options.credentials == CredentialsOption::Omit
					&& name.as_str().eq_ignore_ascii_case("set-cookie")
				{
					return None;
				}

				value
					.to_str()
					.ok()
					.map(|v| (name.to_string(), v.to_string()))
			})
			.collect();

		let empty = status == StatusCode::NO_CONTENT || is_head;

		Ok(FaithResponse {
			inner_body: if empty {
				Body::None
			} else {
				Body::Stream(SharedStream::new(Box::pin(
					response
						.bytes_stream()
						.map(|chunk| chunk.map_err(|err| err.to_string())),
				) as Pin<Box<DynStream>>))
			},
			disturbed: Arc::new(AtomicBool::new(false)),
			headers: headers_vec,
			ok,
			redirected,
			status,
			status_text,
			url,
			version,
		})
	})
}
