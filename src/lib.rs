use napi::bindgen_prelude::*;
use napi_derive::napi;
use reqwest::{Client, Method, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Error, Debug)]
enum FetchError {
    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),
    #[error("Invalid header value: {0}")]
    InvalidHeader(String),
}

impl From<FetchError> for napi::Error {
    fn from(err: FetchError) -> Self {
        napi::Error::new(napi::Status::GenericFailure, err.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[napi(object)]
pub struct FetchOptions {
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<Vec<u8>>,
    pub timeout: Option<f64>,
}

#[derive(Debug)]
struct FetchResponseInner {
    response: Option<Response>,
}

#[napi]
pub struct FetchResponse {
    status: u16,
    status_text: String,
    headers: HashMap<String, String>,
    ok: bool,
    url: String,
    redirected: bool,
    timestamp: f64,
    inner: Option<Arc<Mutex<FetchResponseInner>>>,
    disturbed: AtomicBool,
}

#[napi]
impl FetchResponse {
    #[napi(getter)]
    pub fn status(&self) -> u16 {
        self.status
    }

    #[napi(getter)]
    pub fn status_text(&self) -> String {
        self.status_text.clone()
    }

    #[napi(getter)]
    pub fn headers(&self) -> HashMap<String, String> {
        self.headers.clone()
    }

    #[napi(getter)]
    pub fn ok(&self) -> bool {
        self.ok
    }

    #[napi(getter)]
    pub fn url(&self) -> String {
        self.url.clone()
    }

    #[napi(getter)]
    pub fn redirected(&self) -> bool {
        self.redirected
    }

    #[napi(getter)]
    pub fn timestamp(&self) -> f64 {
        self.timestamp
    }

    /// Check if the response body has been disturbed (read)
    #[napi(getter)]
    pub fn body_used(&self) -> bool {
        self.disturbed.load(Ordering::SeqCst)
    }

    /// Get the response body as a ReadableStream
    #[napi]
    pub fn body(
        &self,
        env: Env,
    ) -> Result<Option<napi::bindgen_prelude::ReadableStream<'_, BufferSlice<'_>>>> {
        // Check if already disturbed
        if self.disturbed.load(Ordering::SeqCst) {
            return Ok(None);
        }

        // Get the inner
        if let Some(inner) = &self.inner {
            // Mark as disturbed
            self.disturbed.store(true, Ordering::SeqCst);

            let inner_clone = Arc::clone(inner);

            // Create a channel for streaming data
            let (tx, rx) = tokio::sync::mpsc::channel(100);

            // Spawn a thread to read bytes synchronously
            std::thread::spawn(move || {
                // Create a Tokio runtime for this thread
                let rt = tokio::runtime::Runtime::new().unwrap();

                let result = rt.block_on(async {
                    let mut inner_guard = inner_clone.lock().await;
                    if let Some(response) = inner_guard.response.take() {
                        match response.bytes().await {
                            Ok(bytes) => Ok(bytes),
                            Err(e) => {
                                // Convert reqwest error to napi error for channel
                                Err(napi::Error::new(
                                    napi::Status::GenericFailure,
                                    e.to_string(),
                                ))
                            }
                        }
                    } else {
                        // Return an error that will be sent through the channel
                        Err(napi::Error::new(
                            napi::Status::GenericFailure,
                            "Response already used".to_string(),
                        ))
                    }
                });

                match result {
                    Ok(bytes) => {
                        let _ = tx.blocking_send(Ok::<Vec<u8>, napi::Error>(bytes.to_vec()));
                    }
                    Err(e) => {
                        // e is already napi::Error
                        let _ = tx.blocking_send(Err::<Vec<u8>, napi::Error>(e));
                    }
                }
            });

            // Create a ReadableStream from the receiver
            let readable_stream = napi::bindgen_prelude::ReadableStream::create_with_stream_bytes(
                &env,
                ReceiverStream::new(rx),
            )?;

            Ok(Some(readable_stream))
        } else {
            Ok(None)
        }
    }

    /// Convert response body to text (UTF-8)
    #[napi]
    pub async fn text(&self) -> Result<String> {
        // Check if already disturbed
        if self.disturbed.load(Ordering::SeqCst) {
            return Err(napi::Error::new(
                napi::Status::GenericFailure,
                "Response already disturbed".to_string(),
            ));
        }

        // Get the inner
        if let Some(inner) = &self.inner {
            // Mark as disturbed
            self.disturbed.store(true, Ordering::SeqCst);

            let mut inner_guard = inner.lock().await;
            if let Some(response) = inner_guard.response.take() {
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
                let text = String::from_utf8(bytes.to_vec())
                    .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
                Ok(text)
            } else {
                Err(napi::Error::new(
                    napi::Status::GenericFailure,
                    "Response already consumed".to_string(),
                ))
            }
        } else {
            Err(napi::Error::new(
                napi::Status::GenericFailure,
                "Response already used".to_string(),
            ))
        }
    }

    /// Get response body as bytes
    #[napi]
    pub async fn bytes(&self) -> Result<Vec<u8>> {
        // Check if already disturbed
        if self.disturbed.load(Ordering::SeqCst) {
            return Err(napi::Error::new(
                napi::Status::GenericFailure,
                "Response already disturbed".to_string(),
            ));
        }

        // Get the inner
        if let Some(inner) = &self.inner {
            // Mark as disturbed
            self.disturbed.store(true, Ordering::SeqCst);

            let mut inner_guard = inner.lock().await;
            if let Some(response) = inner_guard.response.take() {
                let bytes = response
                    .bytes()
                    .await
                    .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))?;
                Ok(bytes.to_vec())
            } else {
                Err(napi::Error::new(
                    napi::Status::GenericFailure,
                    "Response already consumed".to_string(),
                ))
            }
        } else {
            Err(napi::Error::new(
                napi::Status::GenericFailure,
                "Response already used".to_string(),
            ))
        }
    }
}

#[napi]
pub async fn fetch(url: String, options: Option<FetchOptions>) -> Result<FetchResponse> {
    let client = Client::builder()
        .build()
        .map_err(|e| FetchError::Request(e))?;

    let method = options
        .as_ref()
        .and_then(|opts| opts.method.as_ref())
        .map(|m| m.to_uppercase())
        .unwrap_or_else(|| "GET".to_string());

    let method = Method::from_bytes(method.as_bytes())
        .map_err(|_| FetchError::InvalidHeader("Invalid HTTP method".to_string()))?;

    let mut request = client.request(method, &url);

    if let Some(opts) = options.as_ref() {
        if let Some(headers) = &opts.headers {
            for (key, value) in headers {
                request = request.header(key, value);
            }
        }

        if let Some(body) = &opts.body {
            request = request.body(body.clone());
        }

        if let Some(timeout) = opts.timeout {
            request = request.timeout(std::time::Duration::from_millis((timeout * 1000.0) as u64));
        }
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let response = request.send().await.map_err(FetchError::Request)?;

    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("Unknown")
        .to_string();
    let ok = response.status().is_success();
    let url = response.url().to_string();
    let redirected = response.status().is_redirection();

    let headers_map: HashMap<String, String> = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), v.to_string()))
        })
        .collect();

    let inner = FetchResponseInner {
        response: Some(response),
    };

    Ok(FetchResponse {
        status,
        status_text,
        headers: headers_map,
        ok,
        url,
        redirected,
        timestamp,
        inner: Some(Arc::new(Mutex::new(inner))),
        disturbed: AtomicBool::new(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_options_serialization() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let options = FetchOptions {
            method: Some("POST".to_string()),
            headers: Some(headers),
            body: Some(b"test body".to_vec()),
            timeout: Some(30.0),
        };

        let serialized = serde_json::to_string(&options).unwrap();
        let deserialized: FetchOptions = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.method.unwrap(), "POST");
        assert_eq!(deserialized.timeout.unwrap(), 30.0);
    }
}
