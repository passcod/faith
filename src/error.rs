use std::{
	error::Error,
	fmt::{Debug, Display},
};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use strum::{EnumIter, IntoEnumIterator};

/// Faith produces fine-grained errors, but maps them to a few javascript error types for fetch
/// compatibility. The `.code` property on errors thrown from Faith is set to a stable name for each
/// error kind, documented in this comprehensive mapping:
///
/// - JS `AbortError`:
///   - `Aborted` — request was aborted using `signal`
///   - `Timeout` — request timed out
/// - JS `NetworkError`:
///   - `Network` — network error
///   - `Redirect` — when the agent is configured to error on redirects
///   - `ContentLengthOverrun` — a body written with `response.toFile()` exceeded the advertised `Content-Length`
/// - JS `SyntaxError`:
///   - `AddressParse` — IP parse error for `AgentOptions.dns.overrides`
///   - `InvalidIntegrity` — SRI parse error for `RequestInit.integrity`
///   - `JsonParse` — JSON parse error for `response.json()`
///   - `PemParse` — PEM parse error for `AgentOptions.tls.identity` or `AgentOptions.tls.extraRoots`
/// - JS `TypeError`:
///   - `Closed` — a request was made on an agent that has been closed
///   - `InvalidHeader` — invalid header name or value
///   - `InvalidMethod` — invalid HTTP method
///   - `InvalidPath` — a `response.toFile()` destination that does not name a local path
///   - `InvalidUrl` — invalid URL string
///   - `ResponseAlreadyDisturbed` — body already read (mutually exclusive operations)
///   - `ResponseBodyNull` — `response.toFile()` on a response that cannot carry a body
/// - JS generic `Error`:
///   - `BodyStream` — internal stream handling error
///   - `Config` — invalid agent configuration
///   - `FileExists` — a `response.toFile()` write refusing an occupied destination
///   - `FileWrite` — the filesystem refusing a `response.toFile()` write
///   - `IntegrityMismatch` — SRI checksum mismatch (with `RequestInit.integrity`)
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
	Closed,
	Config,
	ContentLengthOverrun,
	FileExists,
	FileWrite,
	IntegrityMismatch,
	InvalidHeader,
	InvalidIntegrity,
	InvalidMethod,
	InvalidPath,
	InvalidUrl,
	JsonParse,
	Network,
	PemParse,
	Redirect,
	ResponseAlreadyDisturbed,
	ResponseBodyNull,
	Timeout,
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
			Self::Closed => "the agent has been closed",
			Self::Config => "invalid agent configuration",
			Self::ContentLengthOverrun => "response body exceeded the advertised Content-Length",
			Self::FileExists => "the destination file already exists",
			Self::FileWrite => "could not write the destination file",
			Self::IntegrityMismatch => "resource integrity check failed",
			Self::InvalidHeader => "invalid header name or value",
			Self::InvalidIntegrity => "invalid integrity value",
			Self::InvalidMethod => "invalid HTTP method",
			Self::InvalidPath => "destination does not name a local path",
			Self::InvalidUrl => "invalid URL",
			Self::JsonParse => "invalid json in response body",
			Self::Network => "network error",
			Self::PemParse => "invalid client certificate or key",
			Self::Redirect => "got a redirect",
			Self::ResponseAlreadyDisturbed => "response body already disturbed",
			Self::ResponseBodyNull => "response cannot carry a body to write",
			Self::Timeout => "timed out",
		}
	}

	fn js_type(self) -> JsErrorType {
		match self {
			Self::BodyStream
			| Self::Config
			| Self::FileExists
			| Self::FileWrite
			| Self::IntegrityMismatch => JsErrorType::GenericError,
			Self::Aborted | Self::Timeout => JsErrorType::NamedError("AbortError"),
			Self::Network | Self::Redirect | Self::ContentLengthOverrun => {
				JsErrorType::NamedError("NetworkError")
			}
			Self::AddressParse | Self::InvalidIntegrity | Self::JsonParse | Self::PemParse => {
				JsErrorType::SyntaxError
			}
			Self::Closed
			| Self::InvalidHeader
			| Self::InvalidMethod
			| Self::InvalidPath
			| Self::InvalidUrl
			| Self::ResponseAlreadyDisturbed
			| Self::ResponseBodyNull => JsErrorType::TypeError,
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

/// Dig a [`FaithError`] back out of an error chain, if one is in there.
///
/// The `error` redirect policy refuses a redirect by handing reqwest a [`FaithError`], which comes
/// back to us wrapped in an error of reqwest's own, so the kind we chose has to be recovered from
/// the source chain to survive as a `code`. Redirect failures reqwest raises on its own account
/// (exhausting the hop limit, an https-only downgrade) carry no [`FaithError`] and so fall through
/// to the generic mapping, which is what tells the two apart.
fn faith_kind_in_chain(err: &(dyn Error + 'static)) -> Option<FaithErrorKind> {
	let mut source = err.source();
	while let Some(e) = source {
		if let Some(faith) = e.downcast_ref::<FaithError>() {
			return Some(faith.kind);
		}
		source = e.source();
	}

	None
}

impl From<reqwest::Error> for FaithError {
	fn from(err: reqwest::Error) -> Self {
		// Always include full error chain for debugging
		let mut msg = format!("{err:?}");
		let mut source = err.source();
		while let Some(e) = source {
			msg.push_str(&format!(" -> {e:?}"));
			source = e.source();
		}

		if err.is_timeout() {
			return FaithError::new(FaithErrorKind::Timeout, Some(msg));
		}

		// A redirect the agent's own policy refused carries the kind we handed reqwest; one reqwest
		// raised on its own account stays a plain network error.
		let kind = err
			.is_redirect()
			.then(|| faith_kind_in_chain(&err))
			.flatten()
			.unwrap_or(FaithErrorKind::Network);

		FaithError::new(kind, Some(msg))
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
