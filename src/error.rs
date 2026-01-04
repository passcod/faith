use std::{
	error::Error,
	fmt::{Debug, Display},
};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use strum::{EnumIter, IntoEnumIterator};

/// Fáith produces fine-grained errors, but maps them to a few javascript error types for fetch
/// compatibility. The `.code` property on errors thrown from Fáith is set to a stable name for each
/// error kind, documented in this comprehensive mapping:
///
/// - JS `AbortError`:
///   - `Aborted` — request was aborted using `signal`
///   - `Timeout` — request timed out
/// - JS `NetworkError`:
///   - `Network` — network error
///   - `Redirect` — when the agent is configured to error on redirects
/// - JS `SyntaxError`:
///   - `JsonParse` — JSON parse error for `response.json()`
///   - `PemParse` — PEM parse error for `AgentOptions.tls.identity`
///   - `Utf8Parse` — UTF8 decoding error for `response.text()`
/// - JS `TypeError`:
///   - `InvalidHeader` — invalid header name or value
///   - `InvalidMethod` — invalid HTTP method
///   - `InvalidUrl` — invalid URL string
///   - `ResponseAlreadyDisturbed` — body already read (mutually exclusive operations)
///   - `ResponseBodyNotAvailable` — body is null or not available
/// - JS generic `Error`:
///   - `BodyStream` — internal stream handling error
///   - `Config` — invalid agent configuration
///   - `RuntimeThread` — failed to start or schedule threads on the internal tokio runtime
///
/// The library exports an `ERROR_CODES` object which has every error code the library throws, and
/// every error thrown also has a `code` property that is set to one of those codes. So you can
/// accurately respond to the exact error kind by checking its code and matching against the right
/// constant from `ERROR_CODES`, instead of doing string matching on the error message, or coarse
/// `instance of` matching.
///
/// Due to technical limitations, when reading a body stream, reads might fail, but that error
/// will not have a `code` property.
#[napi(string_enum)]
#[derive(Debug, Clone, Copy, EnumIter)]
pub enum FaithErrorKind {
	Aborted,
	AddressParse,
	BodyStream,
	Config,
	IntegrityMismatch,
	InvalidHeader,
	InvalidIntegrity,
	InvalidMethod,
	InvalidUrl,
	JsonParse,
	Network,
	PemParse,
	Redirect,
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
			Self::AddressParse => "invalid IP address and/or port",
			Self::BodyStream => "internal response body stream copy error",
			Self::Config => "invalid agent configuration",
			Self::IntegrityMismatch => "resource integrity check failed",
			Self::InvalidHeader => "invalid header name or value",
			Self::InvalidIntegrity => "invalid integrity value",
			Self::InvalidMethod => "invalid HTTP method",
			Self::InvalidUrl => "invalid URL",
			Self::JsonParse => "invalid json in response body",
			Self::Network => "network error",
			Self::PemParse => "invalid client certificate or key",
			Self::Redirect => "got a redirect",
			Self::ResponseAlreadyDisturbed => "response body already disturbed",
			Self::ResponseBodyNotAvailable => "response body not available",
			Self::RuntimeThread => "internal tokio runtime thread error",
			Self::Timeout => "timed out",
			Self::Utf8Parse => "invalid utf-8 in response body",
		}
	}

	fn js_type(self) -> JsErrorType {
		match self {
			Self::BodyStream | Self::Config | Self::IntegrityMismatch | Self::RuntimeThread => {
				JsErrorType::GenericError
			}
			Self::Aborted | Self::Timeout => JsErrorType::NamedError("AbortError"),
			Self::Network | Self::Redirect => JsErrorType::NamedError("NetworkError"),
			Self::AddressParse
			| Self::InvalidIntegrity
			| Self::JsonParse
			| Self::PemParse
			| Self::Utf8Parse => JsErrorType::SyntaxError,
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
		napi::Error::new(napi::Status::GenericFailure, format!("{self}"))
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

impl From<reqwest_middleware::Error> for FaithError {
	fn from(err: reqwest_middleware::Error) -> Self {
		match err {
			reqwest_middleware::Error::Middleware(err) => {
				FaithError::new(FaithErrorKind::Network, Some(err.to_string()))
			}
			reqwest_middleware::Error::Reqwest(err) => err.into(),
		}
	}
}

impl Error for FaithError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		None
	}

	fn description(&self) -> &str {
		"description() is deprecated; use Display"
	}

	fn cause(&self) -> Option<&dyn Error> {
		self.source()
	}
}

impl Display for FaithError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"{:?}: {}",
			self.kind,
			self.message
				.as_deref()
				.unwrap_or_else(|| self.kind.default_message())
		)
	}
}

#[napi]
pub fn error_codes() -> Vec<String> {
	FaithErrorKind::iter()
		.map(|kind| format!("{:?}", kind))
		.collect()
}
