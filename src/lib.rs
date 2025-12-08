use std::{
    fmt::Debug,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use bytes::Bytes;
use futures::{Stream, StreamExt, TryStreamExt};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde_json;
use stream_shared::SharedStream;

#[napi(string_enum)]
#[derive(Debug, Clone, Copy)]
pub enum FaithErrorKind {
    InvalidHeader,
    InvalidMethod,
    InvalidUrl,
    InvalidCredentials,
    InvalidOptions,
    BlockedByPolicy,
    ResponseAlreadyDisturbed,
    ResponseBodyNotAvailable,
    BodyStream,
    JsonParse,
    Timeout,
    PermissionPolicy,
    RequestError,
    Generic,
}

#[derive(Debug)]
pub struct FaithError {
    pub kind: FaithErrorKind,
    pub message: String,
    pub cause: Option<Box<FaithError>>,
}

impl FaithError {
    pub fn new(kind: FaithErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            cause: None,
        }
    }

    pub fn with_cause(kind: FaithErrorKind, message: impl Into<String>, cause: FaithError) -> Self {
        Self {
            kind,
            message: message.into(),
            cause: Some(Box::new(cause)),
        }
    }
}

impl From<reqwest::Error> for FaithError {
    fn from(err: reqwest::Error) -> Self {
        // Map reqwest timeout to Timeout; other reqwest errors map to RequestError.
        if err.is_timeout() {
            FaithError::new(FaithErrorKind::Timeout, err.to_string())
        } else {
            FaithError::new(FaithErrorKind::RequestError, err.to_string())
        }
    }
}

impl From<FaithError> for napi::Error {
    fn from(faith: FaithError) -> Self {
        let status = match faith.kind {
            FaithErrorKind::InvalidHeader
            | FaithErrorKind::InvalidMethod
            | FaithErrorKind::InvalidUrl
            | FaithErrorKind::InvalidCredentials
            | FaithErrorKind::InvalidOptions
            | FaithErrorKind::PermissionPolicy => napi::Status::InvalidArg,
            _ => napi::Status::GenericFailure,
        };

        // Build the message with the kind prefix so wrapper can easily parse `kind` from message
        let message = format!("{:?}: {}", faith.kind, faith.message);

        // Create the napi error with the appropriate status and message
        let mut err = napi::Error::new(status, message);

        // Attach the cause as a proper error cause if present (napi::Error#set_cause)
        if let Some(cause_box) = faith.cause {
            // Convert the boxed faith cause to a napi::Error and set as the cause
            let cause_error: napi::Error = (*cause_box).into();
            // Ignore any errors when setting the cause to avoid panics
            #[allow(unused_must_use)]
            {
                err.set_cause(cause_error);
            }
        }

        err
    }
}

#[napi(object)]
pub struct ErrorCodes {
    pub response_already_disturbed: String,
    pub response_body_not_available: String,
    pub invalid_method: String,
    pub invalid_header: String,
    pub invalid_url: String,
    pub invalid_credentials: String,
    pub invalid_options: String,
    pub permission_policy: String,
    pub timeout: String,
    pub json_parse_error: String,
    pub body_stream_error: String,
    pub request_error: String,
    pub generic_failure: String,
}

#[napi]
pub fn error_codes() -> ErrorCodes {
    ErrorCodes {
        // Use the Auth::Debug-style string for the code to avoid needing
        // separate canonical short codes. This ensures `error.code` is the same
        // as the FaithErrorKind debug string (e.g. `InvalidHeader`, `Timeout`, etc.).
        response_already_disturbed: format!("{:?}", FaithErrorKind::ResponseAlreadyDisturbed),
        response_body_not_available: format!("{:?}", FaithErrorKind::ResponseBodyNotAvailable),
        invalid_method: format!("{:?}", FaithErrorKind::InvalidMethod),
        invalid_header: format!("{:?}", FaithErrorKind::InvalidHeader),
        invalid_url: format!("{:?}", FaithErrorKind::InvalidUrl),
        invalid_credentials: format!("{:?}", FaithErrorKind::InvalidCredentials),
        invalid_options: format!("{:?}", FaithErrorKind::InvalidOptions),
        permission_policy: format!("{:?}", FaithErrorKind::PermissionPolicy),
        timeout: format!("{:?}", FaithErrorKind::Timeout),
        json_parse_error: format!("{:?}", FaithErrorKind::JsonParse),
        body_stream_error: format!("{:?}", FaithErrorKind::BodyStream),
        request_error: format!("{:?}", FaithErrorKind::RequestError),
        generic_failure: format!("{:?}", FaithErrorKind::Generic),
    }
}

#[napi(object)]
pub struct FaithOptions {
    pub method: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub body: Option<Buffer>,
    pub timeout: Option<f64>,
}

type DynStream = dyn Stream<Item = std::result::Result<Bytes, String>> + Send + Sync;

#[derive(Clone)]
enum Body {
    None,
    Stream(SharedStream<Pin<Box<DynStream>>>),
}

impl Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Stream(stream) => {
                let field = f
                    .debug_struct("SharedStream")
                    .field("stats", &stream.stats())
                    .finish_non_exhaustive();
                f.debug_tuple("Stream").field(&field).finish()
            }
        }
    }
}

#[napi]
#[derive(Debug)]
pub struct FaithResponse {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    ok: bool,
    url: String,
    redirected: bool,
    timestamp: f64,
    empty: bool,
    disturbed: AtomicBool,
    inner_body: Body,
}

#[napi]
impl FaithResponse {
    #[napi(getter)]
    pub fn status(&self) -> u16 {
        self.status
    }

    #[napi(getter)]
    pub fn status_text(&self) -> String {
        self.status_text.clone()
    }

    #[napi(getter)]
    pub fn headers(&self) -> Vec<(String, String)> {
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

    /// Check if the response body is empty (e.g. 204 No Content, or HEAD requests)
    #[napi(getter)]
    pub fn body_empty(&self) -> bool {
        self.empty
    }

    /// Get the response body as a ReadableStream
    #[napi]
    pub fn body(
        &self,
        env: Env,
    ) -> Result<Option<napi::bindgen_prelude::ReadableStream<'_, BufferSlice<'_>>>> {
        if self.disturbed.swap(true, Ordering::SeqCst) {
            // In the wrapper code, the body accessor is cached, and then a reference
            // returned if further accessed. We can't do that at this interface. We
            // thus overload Ok(None) (= the method returning `null`) to mean either:
            // - the stream has already been used (cache should be used if present)
            // - the stream is not available (cache should be used if present)
            // - the body is null (the body accessor should return null)
            //   that last case should be checked with body_empty() before calling body()
            return Ok(None);
        }

        match &self.inner_body {
            Body::None => return Ok(None), // the body is legitimately null
            Body::Stream(stream) => {
                let stream = stream.clone();
                let stream = napi::bindgen_prelude::ReadableStream::create_with_stream_bytes(
                    &env,
                    stream.map_err(|err| FaithError::new(FaithErrorKind::BodyStream, err).into()),
                )?;
                Ok(Some(stream))
            }
        }
    }

    fn check_stream_disturbed(&self) -> Result<()> {
        if self.disturbed.swap(true, Ordering::SeqCst) {
            Err(FaithError::new(
                FaithErrorKind::ResponseAlreadyDisturbed,
                "Response already disturbed".to_string(),
            )
            .into())
        } else {
            Ok(())
        }
    }

    /// Underlying efficient response body fetcher.
    ///
    /// Unlike bytes() and co, this grabs all the chunks of the response but doesn't
    /// copy them. Further processing is needed to obtain a Vec<u8> or whatever needed.
    async fn gather(&self) -> Result<Arc<[Bytes]>> {
        // Clone the stream before reading so we don't need &mut self
        let mut response = match &self.inner_body {
            Body::None => return Ok(Default::default()),
            Body::Stream(body) => body.clone(),
        };

        let mut chunks = Vec::new();
        while let Some(chunk) = response
            .next()
            .await
            .transpose()
            .map_err(|e| FaithError::new(FaithErrorKind::BodyStream, e.to_string()))?
        {
            chunks.push(chunk);
        }

        Ok(Arc::from(chunks.into_boxed_slice()))
    }

    /// gather() and then copy into one contiguous buffer
    async fn gather_contiguous(&self) -> Result<Buffer> {
        let body = self.gather().await?;
        let length = body.iter().map(|chunk| chunk.len()).sum();
        let mut bytes = Vec::with_capacity(length);
        for chunk in body.into_iter() {
            bytes.extend_from_slice(chunk);
        }
        Ok(bytes.into())
    }

    /// Get response body as bytes
    ///
    /// This may use up to 2x the amount of memory that the response body takes
    /// when the Response is cloned() and will create a full copy of the data.
    #[napi]
    pub async fn bytes(&self) -> Result<Buffer> {
        self.check_stream_disturbed()?;
        self.gather_contiguous().await
    }

    /// Convert response body to text (UTF-8)
    #[napi]
    pub async fn text(&self) -> Result<String> {
        self.check_stream_disturbed()?;
        let bytes = self.gather_contiguous().await?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| FaithError::new(FaithErrorKind::Generic, e.to_string()).into())
    }

    /// Parse response body as JSON
    #[napi]
    pub async fn json(&self) -> Result<serde_json::Value> {
        self.check_stream_disturbed()?;
        let bytes = self.gather_contiguous().await?;
        let value = serde_json::from_slice(&bytes)
            .map_err(|e| FaithError::new(FaithErrorKind::JsonParse, e.to_string()))?;
        Ok(value)
    }

    /// Create a clone of the response
    ///
    /// Specially, this doesn't set the disturbed flag, so that `body()` or other such
    /// methods can work afterwards. However, it will throw if the body has already
    /// been read from.
    ///
    /// Clones will cache in memory the section of the response body that is read
    /// from one clone and not yet consumed by all others. In the worst case, you can
    /// end up with a copy of the entire response body if you end up not consuming one
    /// of the clones.
    #[napi]
    pub fn clone(&self) -> Result<Self> {
        if self.disturbed.load(Ordering::SeqCst) {
            return Err(FaithError::new(
                FaithErrorKind::ResponseAlreadyDisturbed,
                "Response already disturbed".to_string(),
            )
            .into());
        }

        Ok(Self {
            status: self.status,
            status_text: self.status_text.clone(),
            headers: self.headers.clone(),
            ok: self.ok,
            url: self.url.clone(),
            redirected: self.redirected,
            timestamp: self.timestamp,
            empty: self.empty,
            disturbed: AtomicBool::new(false),
            inner_body: self.inner_body.clone(),
        })
    }
}

#[napi]
pub async fn faith_fetch(url: String, options: Option<FaithOptions>) -> Result<FaithResponse> {
    let client = Client::builder().build().map_err(FaithError::from)?;

    let method = options
        .as_ref()
        .and_then(|opts| opts.method.as_ref())
        .map(|m| m.to_uppercase())
        .unwrap_or_else(|| "GET".to_string());

    let method = Method::from_bytes(method.as_bytes()).map_err(|_| {
        FaithError::new(
            FaithErrorKind::InvalidMethod,
            "Invalid HTTP method".to_string(),
        )
    })?;
    let is_head = method == Method::HEAD;

    // Validate the URL first and ensure no credentials are included in it.
    // Invalid URLs are disallowed in the fetch() spec and should map to a TypeError-like result.
    let parsed_url = reqwest::Url::parse(&url)
        .map_err(|_| FaithError::new(FaithErrorKind::InvalidUrl, "Invalid URL".to_string()))?;

    // Disallow credentials in the URL per the spec (e.g. `https://user:pass@host/`).
    if !parsed_url.username().is_empty() || parsed_url.password().is_some() {
        return Err(FaithError::new(
            FaithErrorKind::InvalidCredentials,
            "URL includes credentials".to_string(),
        )
        .into());
    }

    let mut request = client.request(method, &url);

    if let Some(opts) = options.as_ref() {
        if let Some(headers) = &opts.headers {
            for (key, value) in headers {
                // Validate header name and value before adding to request
                let header_name = HeaderName::from_bytes(key.as_bytes()).map_err(|_| {
                    FaithError::new(
                        FaithErrorKind::InvalidHeader,
                        format!("Invalid header name: {}", key),
                    )
                })?;
                let header_value = HeaderValue::from_str(value).map_err(|_| {
                    FaithError::new(
                        FaithErrorKind::InvalidHeader,
                        format!("Invalid header value: {}", value),
                    )
                })?;
                request = request.header(header_name, header_value);
            }
        }

        if let Some(body) = &opts.body {
            request = request.body(body.as_ref().to_vec());
        }

        if let Some(timeout) = opts.timeout {
            request = request.timeout(std::time::Duration::from_millis((timeout * 1000.0) as u64));
        }
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let response = match request.send().await {
        Ok(resp) => resp,
        Err(e) => {
            if e.is_timeout() {
                return Err(FaithError::new(
                    FaithErrorKind::Timeout,
                    "Request timed out".to_string(),
                )
                .into());
            } else {
                return Err(FaithError::from(e).into());
            }
        }
    };

    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("Unknown")
        .to_string();
    let ok = response.status().is_success();
    let url = response.url().to_string();
    let redirected = response.status().is_redirection();

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
            ) as Pin<Box<DynStream>>))
        },
        status,
        status_text,
        headers: headers_vec,
        ok,
        url,
        redirected,
        timestamp,
        empty,
        disturbed: AtomicBool::new(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_options() {
        let headers = vec![("Content-Type".to_string(), "application/json".to_string())];

        let options = FaithOptions {
            method: Some("POST".to_string()),
            headers: Some(headers),
            body: Some(Buffer::from(b"test body".to_vec())),
            timeout: Some(30.0),
        };

        assert_eq!(options.method.unwrap(), "POST");
        assert_eq!(options.timeout.unwrap(), 30.0);
        assert_eq!(options.body.unwrap().as_ref(), b"test body");
    }
}
