use std::{
	error::Error,
	fmt::{Debug, Display},
};

use strum::{EnumIter, IntoEnumIterator};

/// The kind of a [`FaithError`], which is also the stable code the error reports.
///
/// Callers match on the kind rather than on the message: the kind is the API, and the message is
/// for humans. Every kind here is reachable, each one naming a failure some request can produce.
///
/// This is the one definition of the set, on either surface. The Node surface hands JavaScript the
/// codes through [`error_codes`], which reads them from here, so the exported `ERROR_CODES` map and
/// the errors themselves cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
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
	InvalidCompression,
	InvalidHeader,
	InvalidIntegrity,
	InvalidMethod,
	InvalidPath,
	InvalidUrl,
	JsonParse,
	MissingContentType,
	Network,
	PemParse,
	Redirect,
	ResponseAlreadyDisturbed,
	ResponseBodyNull,
	Timeout,
}

impl FaithErrorKind {
	/// The stable name callers match on.
	pub fn code(self) -> String {
		format!("{self:?}")
	}

	pub(crate) fn default_message(self) -> &'static str {
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
			Self::InvalidCompression => "invalid request body compression",
			Self::InvalidHeader => "invalid header name or value",
			Self::InvalidIntegrity => "invalid integrity value",
			Self::InvalidMethod => "invalid HTTP method",
			Self::InvalidPath => "destination does not name a local path",
			Self::InvalidUrl => "invalid URL",
			Self::JsonParse => "invalid json in response body",
			Self::MissingContentType => "a QUERY request with a body requires a Content-Type",
			Self::Network => "network error",
			Self::PemParse => "invalid client certificate or key",
			Self::Redirect => "got a redirect",
			Self::ResponseAlreadyDisturbed => "response body already disturbed",
			Self::ResponseBodyNull => "response cannot carry a body to write",
			Self::Timeout => "timed out",
		}
	}
}

/// Every error code the library reports, in declaration order.
///
/// The Node surface exports this as `ERROR_CODES`; generating it from the kinds themselves is what
/// keeps the exported map and the errors from drifting.
pub fn error_codes() -> Vec<String> {
	FaithErrorKind::iter().map(FaithErrorKind::code).collect()
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
}

impl From<FaithErrorKind> for FaithError {
	fn from(kind: FaithErrorKind) -> Self {
		Self {
			kind,
			message: None,
		}
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

/// A conversion that cannot fail still has to satisfy the bound on a target, and this is how it
/// does: there is no value to convert.
impl From<std::convert::Infallible> for FaithError {
	fn from(never: std::convert::Infallible) -> Self {
		match never {}
	}
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_code_is_distinct_and_named() {
		let codes = error_codes();
		let unique: std::collections::BTreeSet<_> = codes.iter().collect();
		assert_eq!(unique.len(), codes.len(), "two kinds report the same code");
		assert!(codes.iter().all(|code| !code.is_empty()));
	}

	#[test]
	fn a_message_is_prefixed_with_the_code_it_reports() {
		for kind in FaithErrorKind::iter() {
			let code = kind.code();
			let rendered = FaithError::from(kind).to_string();
			assert!(
				rendered.starts_with(&format!("{code}: ")),
				"{rendered} does not lead with {code}"
			);
		}
	}

	#[test]
	fn a_kind_without_a_message_falls_back_to_its_own() {
		let err = FaithError::from(FaithErrorKind::Closed);
		assert_eq!(err.to_string(), "Closed: the agent has been closed");

		let err = FaithError::new(FaithErrorKind::Closed, Some("gone"));
		assert_eq!(err.to_string(), "Closed: gone");
	}
}
