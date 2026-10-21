//! The crate's errors.

use std::{fmt, io};

use crate::Source;

/// Why revocation data could not be fetched, verified, or loaded.
///
/// These never reach a TLS handshake: a failed update leaves the data already held in force.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
	/// A request to Remote Settings or its attachment host failed.
	Http(reqwest::Error),
	/// A response or cache file was larger than this crate accepts.
	TooLarge,
	/// A response or cache file was not the JSON expected.
	Json(serde_json::Error),
	/// A collection's content signature did not verify.
	Signature(&'static str),
	/// A collection was signed earlier than the one already held.
	Rollback,
	/// A CRLite filter did not match the hash or size in its signed record.
	FilterHash,
	/// A CRLite filter could not be decoded.
	#[cfg(feature = "crlite")]
	Filter,
	/// The collection did not have the shape this crate expects.
	Collection(&'static str),
	/// Reading or writing the cache failed.
	Io(io::Error),
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Http(err) => write!(f, "fetching revocation data: {err}"),
			Self::TooLarge => f.write_str("revocation data larger than accepted"),
			Self::Json(err) => write!(f, "decoding revocation data: {err}"),
			Self::Signature(why) => write!(f, "content signature: {why}"),
			Self::Rollback => f.write_str("collection is older than the data already held"),
			Self::FilterHash => f.write_str("CRLite filter does not match its signed record"),
			#[cfg(feature = "crlite")]
			Self::Filter => f.write_str("CRLite filter does not decode"),
			Self::Collection(why) => write!(f, "unexpected collection: {why}"),
			Self::Io(err) => write!(f, "revocation cache: {err}"),
		}
	}
}

impl std::error::Error for Error {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Self::Http(err) => Some(err),
			Self::Json(err) => Some(err),
			Self::Io(err) => Some(err),
			_ => None,
		}
	}
}

impl From<reqwest::Error> for Error {
	fn from(err: reqwest::Error) -> Self {
		Self::Http(err)
	}
}

impl From<serde_json::Error> for Error {
	fn from(err: serde_json::Error) -> Self {
		Self::Json(err)
	}
}

impl From<io::Error> for Error {
	fn from(err: io::Error) -> Self {
		Self::Io(err)
	}
}

/// The error a refused certificate carries, inside rustls's
/// [`CertificateError::Other`](rustls::CertificateError::Other).
///
/// Its message says the certificate is revoked and names the source that refused it, so the
/// error a client surfaces for the failed connection does too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revoked {
	/// The source that listed the certificate.
	pub source: Source,
}

impl fmt::Display for Revoked {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "certificate revoked ({})", self.source)
	}
}

impl std::error::Error for Revoked {}
