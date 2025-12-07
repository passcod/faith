use napi::bindgen_prelude::*;
use napi_derive::napi;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

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

#[derive(Debug, Serialize, Deserialize)]
#[napi(object)]
pub struct FetchResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub ok: bool,
    pub body: Vec<u8>,
    pub url: String,
    pub redirected: bool,
    pub timestamp: f64,
}

#[napi]
impl FetchResponse {
    /// Convert response body to text (UTF-8)
    #[napi]
    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.body.clone())
            .map_err(|e| napi::Error::new(napi::Status::GenericFailure, e.to_string()))
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

    let body_bytes = response.bytes().await.map_err(FetchError::Request)?;
    let body_vec = body_bytes.to_vec();

    Ok(FetchResponse {
        status,
        status_text,
        headers: headers_map,
        ok,
        body: body_vec,
        url,
        redirected,
        timestamp,
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

    #[test]
    fn test_fetch_response_serialization() {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let response = FetchResponse {
            status: 200,
            status_text: "OK".to_string(),
            headers,
            ok: true,
            body: b"{\"test\": true}".to_vec(),
            url: "https://example.com".to_string(),
            redirected: false,
            timestamp: 1234567890.0,
        };

        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: FetchResponse = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.status, 200);
        assert_eq!(deserialized.ok, true);
        assert_eq!(deserialized.url, "https://example.com");
    }
}
