use std::{
    pin::Pin,
    sync::{Arc, atomic::AtomicBool},
};

use futures::StreamExt;
use napi_derive::napi;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Method, StatusCode};
use stream_shared::SharedStream;

use crate::{
    async_task::{Async, FaithAsyncResult},
    body::{Body, DynStream},
    error::{FaithError, FaithErrorKind},
    options::{FaithOptions, FaithOptionsAndBody},
    response::FaithResponse,
};

#[napi]
pub fn faith_fetch(url: String, options: FaithOptionsAndBody) -> Async<FaithResponse> {
    let (options, agent, body) = FaithOptions::extract(options);
    FaithAsyncResult::run(move || {
        let url = url.clone();
        let options = options.clone();
        let body = body.clone();
        let agent = agent.clone();
        async move {
            let method = options
                .method
                .map(|m| m.to_uppercase())
                .unwrap_or_else(|| "GET".to_string());

            let method =
                Method::from_bytes(method.as_bytes()).map_err(|_| FaithErrorKind::InvalidMethod)?;
            let is_head = method == Method::HEAD;

            let parsed_url = reqwest::Url::parse(&url).map_err(|_| FaithErrorKind::InvalidUrl)?;

            let mut request = agent.client.request(method, parsed_url);

            if let Some(headers) = &options.headers {
                for (key, value) in headers {
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

            if let Some(timeout) = options.timeout {
                request =
                    request.timeout(std::time::Duration::from_millis((timeout * 1000.0) as u64));
            }

            let response = request.send().await?;

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
                    )
                        as Pin<Box<DynStream>>))
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
        }
    })
}
