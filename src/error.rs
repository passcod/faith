use std::fmt::Debug;

use napi::bindgen_prelude::*;
use napi_derive::napi;

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
        let code = format!("{:?}", self.kind);
        let unk = match self.kind.js_type() {
            JsErrorType::TypeError => JsTypeError::from(self.into_napi()).into_unknown(*env),
            JsErrorType::SyntaxError => JsSyntaxError::from(self.into_napi()).into_unknown(*env),
            JsErrorType::GenericError => JsError::from(self.into_napi()).into_unknown(*env),
        };

        // we do this manually instead of using the TryFrom so we can return the untouched Unknown if we fail
        let Ok(typ) = unk.get_type() else { return unk };
        if typ != ValueType::Object {
            return unk;
        }
        // SAFETY: we have verified that this value is an Object
        let Ok(mut obj) = (unsafe { unk.cast::<Object>() }) else {
            return unk;
        };

        let _ = obj.set("code", code);
        obj.into_unknown(env).unwrap_or(unk)
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
