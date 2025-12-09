use std::{
    fmt::Debug,
    pin::Pin,
    result::Result,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures::{Stream, StreamExt, TryStreamExt};
use napi::{
    ScopedTask,
    bindgen_prelude::*,
    sys::{napi_env, napi_value},
};
use napi_derive::napi;
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde_json;
use stream_shared::SharedStream;
use tokio::runtime::{Handle, Runtime};

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
    Utf8Parse,
    Timeout,
    PermissionPolicy,
    Network,
    RuntimeThread,
    Generic,
}

#[derive(Debug, Clone, Copy)]
enum JsErrorType {
    TypeError,
    SyntaxError,
    GenericError,
}

impl FaithErrorKind {
    fn default_message(self) -> &'static str {
        match self {
            Self::InvalidHeader => "invalid header name or value",
            Self::InvalidMethod => "invalid HTTP method",
            Self::InvalidUrl => "invalid URL",
            Self::InvalidCredentials => "invalid credentials",
            Self::InvalidOptions => "invalid fetch options",
            Self::BlockedByPolicy => "blocked by network policy",
            Self::ResponseAlreadyDisturbed => "response body already disturbed",
            Self::ResponseBodyNotAvailable => "response body not available",
            Self::BodyStream => "internal response body stream copy error",
            Self::JsonParse => "invalid json in response body",
            Self::Utf8Parse => "invalid utf-8 in response body",
            Self::Timeout => "timed out",
            Self::PermissionPolicy => "not permitted",
            Self::Network => "network error",
            Self::RuntimeThread => "internal tokio runtime thread error",
            Self::Generic => "fetch error",
        }
    }

    fn js_type(self) -> JsErrorType {
        match self {
            Self::InvalidHeader
            | Self::InvalidMethod
            | Self::InvalidUrl
            | Self::InvalidCredentials
            | Self::InvalidOptions
            | Self::PermissionPolicy
            | Self::ResponseAlreadyDisturbed
            | Self::ResponseBodyNotAvailable => JsErrorType::TypeError,
            Self::JsonParse | Self::Utf8Parse => JsErrorType::SyntaxError,
            _ => JsErrorType::GenericError,
        }
    }
}

impl From<FaithErrorKind> for FaithError {
    fn from(kind: FaithErrorKind) -> Self {
        Self {
            kind,
            message: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FaithError {
    pub kind: FaithErrorKind,
    pub message: Option<String>,
}

impl FaithError {
    pub fn new(kind: FaithErrorKind, message: Option<impl Into<String>>) -> Self {
        Self {
            kind,
            message: message.map(|m| m.into()),
        }
    }

    // we make this explicit instead of adding a From<> so that we can't accidentally do it
    pub fn into_napi(self) -> napi::Error {
        napi::Error::new(
            napi::Status::GenericFailure,
            format!(
                "{:?}: {}",
                self.kind,
                self.message
                    .unwrap_or_else(|| self.kind.default_message().to_owned())
            ),
        )
    }

    // whenever possible, we should prefer to use this so that the error types are correct
    pub fn into_js_error<'env>(self, env: &'env Env) -> Unknown<'env> {
        match self.kind.js_type() {
            JsErrorType::TypeError => JsTypeError::from(self.into_napi()).into_unknown(*env),
            JsErrorType::SyntaxError => JsSyntaxError::from(self.into_napi()).into_unknown(*env),
            JsErrorType::GenericError => JsError::from(self.into_napi()).into_unknown(*env),
        }
    }
}

impl From<reqwest::Error> for FaithError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            FaithError::new(FaithErrorKind::Timeout, Some(err.to_string()))
        } else {
            FaithError::new(FaithErrorKind::Network, Some(err.to_string()))
        }
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
        request_error: format!("{:?}", FaithErrorKind::Network),
        generic_failure: format!("{:?}", FaithErrorKind::Generic),
    }
}

#[napi(object)]
pub struct FaithOptionsAndBody {
    pub method: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub body: Option<Buffer>,
    pub timeout: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct FaithOptions {
    pub method: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub timeout: Option<f64>,
}

impl FaithOptions {
    fn extract(opts: Option<FaithOptionsAndBody>) -> (Self, Option<Arc<Buffer>>) {
        match opts {
            None => (Self::default(), None),
            Some(opts) => (
                Self {
                    method: opts.method,
                    headers: opts.headers,
                    timeout: opts.timeout,
                },
                opts.body.map(Arc::new),
            ),
        }
    }
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

impl Clone for FaithResponse {
    fn clone(&self) -> Self {
        Self {
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
        }
    }
}

#[derive(Clone, Debug)]
pub struct Value(serde_json::Value);

impl TypeName for Value {
    fn type_name() -> &'static str {
        "unknown"
    }

    fn value_type() -> ValueType {
        ValueType::Unknown
    }
}

impl ToNapiValue for Value {
    unsafe fn to_napi_value(env: napi_env, val: Self) -> Result<napi_value, napi::Error> {
        unsafe { serde_json::Value::to_napi_value(env, val.0) }
    }
}

pub type Async<T> = AsyncTask<FaithAsyncResult<T>>;
pub struct FaithAsyncResult<T>(Pin<Box<dyn Future<Output = Result<T, FaithError>> + Send>>)
where
    T: Send + ToNapiValue + TypeName + 'static;

impl<T> FaithAsyncResult<T>
where
    T: Send + ToNapiValue + TypeName + 'static,
{
    pub fn run<F, U>(f: F) -> AsyncTask<Self>
    where
        F: Fn() -> U + Send + 'static,
        U: Future<Output = Result<T, FaithError>> + Send + 'static,
    {
        AsyncTask::new(Self(Box::pin(f())))
    }
}

impl<'env, T> ScopedTask<'env> for FaithAsyncResult<T>
where
    T: Send + ToNapiValue + TypeName + 'static,
{
    type Output = Result<T, FaithError>;
    type JsValue = T;

    fn compute(&mut self) -> Result<Self::Output, napi::Error> {
        match Handle::try_current() {
            Ok(handle) => Ok(handle.block_on(&mut self.0)),
            Err(err) if err.is_missing_context() => {
                let rt = Runtime::new().map_err(|err| {
                    FaithError::new(FaithErrorKind::RuntimeThread, Some(err.to_string()))
                        .into_napi()
                })?;
                Ok(rt.block_on(&mut self.0))
            }
            Err(err) => Err(
                FaithError::new(FaithErrorKind::RuntimeThread, Some(err.to_string())).into_napi(),
            ),
        }
    }

    fn resolve(
        &mut self,
        env: &'env Env,
        output: Self::Output,
    ) -> Result<Self::JsValue, napi::Error> {
        match output {
            Ok(t) => Ok(t),
            Err(err) => Err(napi::Error::from(err.into_js_error(env))),
        }
    }

    fn reject(&mut self, _env: &'env Env, err: Error) -> Result<Self::JsValue, napi::Error> {
        debug_assert!(false, "FaithAsyncResult::reject should be unreachable");
        Err(err)
    }

    fn finally(self, _: Env) -> Result<(), napi::Error> {
        drop(self.0);
        Ok(())
    }
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
    ) -> Result<Option<napi::bindgen_prelude::ReadableStream<'_, BufferSlice<'_>>>, napi::Error>
    {
        // we mark the body as disturbed, but we still allow reading it through here
        // as essentially, the body() can be accessed many times as the same stream
        let _ = self.check_stream_disturbed();

        match &self.inner_body {
            Body::None => Ok(None),
            Body::Stream(stream) => {
                let stream = stream.clone();
                let stream = napi::bindgen_prelude::ReadableStream::create_with_stream_bytes(
                    &env,
                    stream.map_err(|err| {
                        FaithError::new(FaithErrorKind::BodyStream, Some(err)).into_napi()
                    }),
                )?;
                Ok(Some(stream))
            }
        }
    }

    fn check_stream_disturbed(&self) -> Result<(), FaithError> {
        if self.disturbed.swap(true, Ordering::SeqCst) {
            Err(FaithErrorKind::ResponseAlreadyDisturbed.into())
        } else {
            Ok(())
        }
    }

    /// Underlying efficient response body fetcher.
    ///
    /// Unlike bytes() and co, this grabs all the chunks of the response but doesn't
    /// copy them. Further processing is needed to obtain a Vec<u8> or whatever needed.
    async fn gather(&self) -> Result<Arc<[Bytes]>, FaithError> {
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
            .map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)))?
        {
            chunks.push(chunk);
        }

        Ok(Arc::from(chunks.into_boxed_slice()))
    }

    /// gather() and then copy into one contiguous buffer
    async fn gather_contiguous(&self) -> Result<Buffer, FaithError> {
        let body = self.gather().await?;
        let length = body.iter().map(|chunk| chunk.len()).sum();
        let mut bytes = Vec::with_capacity(length);
        for chunk in body.into_iter() {
            bytes.extend_from_slice(chunk);
        }
        Ok(bytes.into())
    }

    #[napi]
    pub fn testing_unk(&self, test: String) -> Async<bool> {
        FaithAsyncResult::run(move || {
            let test = test.clone();
            async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok(test.is_empty())
            }
        })
    }

    /// Get response body as bytes
    ///
    /// This may use up to 2x the amount of memory that the response body takes
    /// when the Response is cloned() and will create a full copy of the data.
    #[napi]
    pub fn bytes(&self) -> Async<Buffer> {
        let this = Clone::clone(&*self);
        FaithAsyncResult::run(move || {
            let this = Clone::clone(&this);
            async move {
                this.check_stream_disturbed()?;
                let buf = this.gather_contiguous().await?;
                Ok(buf)
            }
        })
    }

    /// Convert response body to text (UTF-8)
    #[napi]
    pub fn text(&self) -> Async<String> {
        let this = Clone::clone(&*self);
        FaithAsyncResult::run(move || {
            let this = Clone::clone(&this);
            async move {
                this.check_stream_disturbed()?;
                let bytes = this.gather_contiguous().await?;
                String::from_utf8(bytes.to_vec()).map_err(|e| {
                    FaithError::new(FaithErrorKind::Generic, Some(e.to_string())).into()
                })
            }
        })
    }

    /// Parse response body as JSON
    #[napi]
    pub fn json(&self) -> Async<Value> {
        let this = Clone::clone(&*self);
        FaithAsyncResult::run(move || {
            let this = Clone::clone(&this);
            async move {
                this.check_stream_disturbed()?;
                let bytes = this.gather_contiguous().await?;
                let value = serde_json::from_slice(&bytes)
                    .map_err(|e| FaithError::new(FaithErrorKind::JsonParse, Some(e.to_string())))?;
                Ok(Value(value))
            }
        })
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
    pub fn clone(&self) -> Result<Self, napi::Error> {
        if self.disturbed.load(Ordering::SeqCst) {
            // FIXME: figure out how to return a TypeError here while maintaining non-async
            return Err(FaithError::from(FaithErrorKind::ResponseAlreadyDisturbed).into_napi());
        }

        Ok(Clone::clone(self))
    }
}

#[napi]
pub fn faith_fetch(url: String, options: Option<FaithOptionsAndBody>) -> Async<FaithResponse> {
    let (options, body) = FaithOptions::extract(options);
    FaithAsyncResult::run(move || {
        let url = url.clone();
        let options = options.clone();
        let body = body.clone();
        async move {
            let client = Client::builder().build().map_err(FaithError::from)?;

            let method = options
                .method
                .map(|m| m.to_uppercase())
                .unwrap_or_else(|| "GET".to_string());

            let method =
                Method::from_bytes(method.as_bytes()).map_err(|_| FaithErrorKind::InvalidMethod)?;
            let is_head = method == Method::HEAD;

            let parsed_url = reqwest::Url::parse(&url).map_err(|_| FaithErrorKind::InvalidUrl)?;

            let mut request = client.request(method, parsed_url);

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

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            let response = request.send().await?;

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
                    )
                        as Pin<Box<DynStream>>))
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
    })
}
