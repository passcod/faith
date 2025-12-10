use std::fmt::Debug;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use strum::{EnumIter, IntoEnumIterator};

#[napi(string_enum)]
#[derive(Debug, Clone, Copy, EnumIter)]
pub enum FaithErrorKind {
    Aborted,
    BodyStream,
    InvalidHeader,
    InvalidMethod,
    InvalidUrl,
    JsonParse,
    Network,
    ResponseAlreadyDisturbed,
    ResponseBodyNotAvailable,
    RuntimeThread,
    Timeout,
    Utf8Parse,
}

#[derive(Debug, Clone, Copy)]
enum JsErrorType {
    GenericError,
    NamedError(&'static str),
    SyntaxError,
    TypeError,
}

impl FaithErrorKind {
    fn default_message(self) -> &'static str {
        match self {
            Self::Aborted => "the request was aborted",
            Self::BodyStream => "internal response body stream copy error",
            Self::InvalidHeader => "invalid header name or value",
            Self::InvalidMethod => "invalid HTTP method",
            Self::InvalidUrl => "invalid URL",
            Self::JsonParse => "invalid json in response body",
            Self::Network => "network error",
            Self::ResponseAlreadyDisturbed => "response body already disturbed",
            Self::ResponseBodyNotAvailable => "response body not available",
            Self::RuntimeThread => "internal tokio runtime thread error",
            Self::Timeout => "timed out",
            Self::Utf8Parse => "invalid utf-8 in response body",
        }
    }

    fn js_type(self) -> JsErrorType {
        match self {
            Self::BodyStream | Self::RuntimeThread => JsErrorType::GenericError,
            Self::Aborted | Self::Timeout => JsErrorType::NamedError("AbortError"),
            Self::Network => JsErrorType::NamedError("NetworkError"),
            Self::JsonParse | Self::Utf8Parse => JsErrorType::SyntaxError,
            Self::InvalidHeader
            | Self::InvalidMethod
            | Self::InvalidUrl
            | Self::ResponseAlreadyDisturbed
            | Self::ResponseBodyNotAvailable => JsErrorType::TypeError,
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
        self.to_napi()
    }
    fn to_napi(&self) -> napi::Error {
        napi::Error::new(
            napi::Status::GenericFailure,
            format!(
                "{:?}: {}",
                self.kind,
                self.message
                    .as_deref()
                    .unwrap_or_else(|| self.kind.default_message())
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
            JsErrorType::NamedError(name) => env
                .create_error(self.to_napi())
                .and_then(|mut err| {
                    err.set_named_property("name", name)?;
                    Ok(err)
                })
                .and_then(|err| err.into_unknown(env))
                .unwrap_or_else(|_| JsError::from(self.into_napi()).into_unknown(*env)),
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

#[napi]
pub fn error_codes() -> Vec<String> {
    FaithErrorKind::iter()
        .map(|kind| format!("{:?}", kind))
        .collect()
}
