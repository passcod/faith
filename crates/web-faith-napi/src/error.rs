use napi::bindgen_prelude::*;
use napi_derive::napi;
pub use web_faith::error::{FaithError, FaithErrorKind};

#[derive(Debug, Clone, Copy)]
enum JsErrorType {
	GenericError,
	NamedError(&'static str),
	SyntaxError,
	TypeError,
}

/// Which JavaScript error class a kind is thrown as.
///
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
///   - `InvalidCompression` — `RequestInit.compress` naming no coding Faith can compress in
///   - `InvalidHeader` — invalid header name or value
///   - `InvalidMethod` — invalid HTTP method
///   - `InvalidPath` — a `response.toFile()` destination that does not name a local path
///   - `InvalidUrl` — invalid URL string
///   - `MissingContentType` — a `QUERY` request carrying a body with no `Content-Type` to describe it
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
fn js_type(kind: FaithErrorKind) -> JsErrorType {
	use FaithErrorKind as K;
	match kind {
		K::BodyStream | K::Config | K::FileExists | K::FileWrite | K::IntegrityMismatch => {
			JsErrorType::GenericError
		}
		K::Aborted | K::Timeout => JsErrorType::NamedError("AbortError"),
		K::Network | K::Redirect | K::ContentLengthOverrun => {
			JsErrorType::NamedError("NetworkError")
		}
		K::AddressParse | K::InvalidIntegrity | K::JsonParse | K::PemParse => {
			JsErrorType::SyntaxError
		}
		K::Closed
		| K::InvalidCompression
		| K::InvalidHeader
		| K::InvalidMethod
		| K::InvalidPath
		| K::InvalidUrl
		| K::MissingContentType
		| K::ResponseAlreadyDisturbed
		| K::ResponseBodyNull => JsErrorType::TypeError,
	}
}

/// Throwing a [`FaithError`] into JavaScript.
///
/// The error itself is the client's, and knows nothing of napi; turning one into a value V8 can
/// carry is this crate's business, which is why it arrives as an extension rather than as methods
/// on the error.
pub trait FaithErrorExt {
	/// Convert to a napi error.
	///
	/// This is explicit rather than a `From` impl so that it cannot happen by accident, losing the
	/// error class and code that [`Self::into_js_error`] preserves.
	fn into_napi(self) -> napi::Error;

	fn to_napi(&self) -> napi::Error;

	/// Whenever possible, prefer this so that the error types are correct.
	fn into_js_error<'env>(self, env: &'env Env) -> Unknown<'env>;
}

impl FaithErrorExt for FaithError {
	fn into_napi(self) -> napi::Error {
		self.to_napi()
	}

	fn to_napi(&self) -> napi::Error {
		napi::Error::new(napi::Status::GenericFailure, format!("{self}"))
	}

	fn into_js_error<'env>(self, env: &'env Env) -> Unknown<'env> {
		let code = self.kind.code();
		let unk = match js_type(self.kind) {
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

#[napi]
pub fn error_codes() -> Vec<String> {
	web_faith::error_codes()
}
