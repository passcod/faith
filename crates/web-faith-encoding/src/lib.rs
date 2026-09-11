//! HTTP content coding for request and response bodies.
//!
//! Currently supports:
//!
//! - gzip ([RFC 1952](https://www.rfc-editor.org/rfc/rfc1952))
//! - deflate, in its zlib-wrapped form ([RFC 1950](https://www.rfc-editor.org/rfc/rfc1950)), like
//!   browsers
//! - brotli ([RFC 7932](https://www.rfc-editor.org/rfc/rfc7932))
//! - zstd ([RFC 8878](https://www.rfc-editor.org/rfc/rfc8878))
//!
//! # Requests
//!
//! - Use [`compress_buffer`] or [`compress_stream`] to apply a coding to a request body.
//! - Use [`layer_content_encoding`] to build the `Content-Encoding` header that describes it.
//!
//! ```
//! use web_faith_encoding::{Coding, request::{compress_buffer, layer_content_encoding}};
//!
//! # async fn example() {
//! let body = b"the quick brown fox".repeat(8);
//! let compressed = compress_buffer(&body, Coding::Gzip).await.expect("gzip compresses");
//! assert!(compressed.len() < body.len());
//!
//! // The request declared nothing, so the applied coding stands alone.
//! assert_eq!(layer_content_encoding(None, Coding::Gzip), "gzip");
//! // Otherwise it is added last, being applied on top of what was already there.
//! assert_eq!(layer_content_encoding(Some("br"), Coding::Gzip), "br, gzip");
//! # }
//! ```
//!
//! # Responses
//!
//! - Use [`AcceptEncoding`] to parse the advertised supported coding set from the request.
//! - Use [`decision`] to compute which decoder to use for the response's body, if any.
//! - Use [`decode_stream`] to wrap the body in that decoder.
//! - Use [`strip_decoded_headers`] to remove the headers that described the encoded bytes.
//!
//! ```
//! use http::{HeaderMap, HeaderValue};
//! use web_faith_encoding::{
//!     Coding,
//!     response::{AcceptEncoding, DEFAULT_ACCEPT_ENCODING, decision},
//! };
//!
//! let accept = AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING);
//!
//! let mut headers = HeaderMap::new();
//! headers.insert("content-encoding", HeaderValue::from_static("gzip"));
//!
//! // What the response declared, against what the request accepted.
//! assert_eq!(decision(&headers, &accept), Some(Coding::Gzip));
//! ```
//!
//! [`compress_buffer`]: request::compress_buffer
//! [`compress_stream`]: request::compress_stream
//! [`layer_content_encoding`]: request::layer_content_encoding
//! [`AcceptEncoding`]: response::AcceptEncoding
//! [`decision`]: response::decision
//! [`decode_stream`]: response::decode_stream
//! [`strip_decoded_headers`]: response::strip_decoded_headers

#![deny(missing_docs)]
// Lets docs.rs label each item with the feature or platform it needs.
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod request;
pub mod response;

/// A content coding.
///
/// The four this crate decodes, and [`Other`](Self::Other) for any token it does not. Marked
/// non-exhaustive: a coding that becomes standard should not be a breaking change here.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Coding {
	/// [RFC 1952](https://www.rfc-editor.org/rfc/rfc1952).
	Gzip,
	/// In its zlib-wrapped form ([RFC 1950](https://www.rfc-editor.org/rfc/rfc1950)), like browsers.
	Deflate,
	/// [RFC 7932](https://www.rfc-editor.org/rfc/rfc7932).
	Brotli,
	/// [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878).
	Zstd,
	/// A coding named on the wire that this crate does not decode, lowercased.
	///
	/// Includes `identity`, which means the absence of a coding rather than one to apply.
	Other(String),
}

impl Coding {
	/// Match the `compress` option's value, given as a coding's wire token.
	///
	/// Matches the four documented tokens exactly. [`Self::from_token`] reads off the wire and so
	/// takes a token as loosely as HTTP writes it.
	// spec:ENC#compressing-a-request-body
	pub fn from_option(value: &str) -> Option<Self> {
		match value {
			"gzip" => Some(Self::Gzip),
			"deflate" => Some(Self::Deflate),
			"br" => Some(Self::Brotli),
			"zstd" => Some(Self::Zstd),
			_ => None,
		}
	}

	/// The wire token for this coding in a `Content-Encoding`.
	pub fn token(&self) -> &str {
		match self {
			Self::Gzip => "gzip",
			Self::Deflate => "deflate",
			Self::Brotli => "br",
			Self::Zstd => "zstd",
			Self::Other(token) => token,
		}
	}

	/// Read a content-coding token, case-insensitively.
	///
	/// Anything this crate does not decode, `identity` included, becomes
	/// [`Other`](Self::Other); see [`is_supported`](Self::is_supported).
	pub fn from_token(token: &str) -> Self {
		let token = token.trim();
		if token.eq_ignore_ascii_case("gzip") || token.eq_ignore_ascii_case("x-gzip") {
			Self::Gzip
		} else if token.eq_ignore_ascii_case("deflate") {
			Self::Deflate
		} else if token.eq_ignore_ascii_case("br") {
			Self::Brotli
		} else if token.eq_ignore_ascii_case("zstd") {
			Self::Zstd
		} else {
			Self::Other(token.to_ascii_lowercase())
		}
	}

	/// Whether this crate can decode this coding.
	pub fn is_supported(&self) -> bool {
		!matches!(self, Self::Other(_))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_compress_option_names_a_coding_by_its_wire_token() {
		assert_eq!(Coding::from_option("gzip"), Some(Coding::Gzip));
		assert_eq!(Coding::from_option("deflate"), Some(Coding::Deflate));
		assert_eq!(Coding::from_option("br"), Some(Coding::Brotli));
		assert_eq!(Coding::from_option("zstd"), Some(Coding::Zstd));
	}
	#[test]
	fn the_compress_option_matches_its_tokens_exactly() {
		// Loose on the wire, exact as an API: `x-gzip` and a shouted token are read off a
		// `Content-Encoding` but refused as option values.
		assert_eq!(Coding::from_token("x-gzip"), Coding::Gzip);
		assert_eq!(Coding::from_option("x-gzip"), None);
		assert_eq!(Coding::from_token("GZIP"), Coding::Gzip);
		assert_eq!(Coding::from_option("GZIP"), None);
		assert_eq!(Coding::from_option(" gzip"), None);
		assert_eq!(Coding::from_option("brotli"), None);
		assert_eq!(Coding::from_option("identity"), None);
		assert_eq!(Coding::from_option(""), None);
	}

	#[test]
	fn a_coding_this_crate_cannot_decode_is_still_named() {
		let identity = Coding::from_token("identity");
		assert_eq!(identity, Coding::Other("identity".into()));
		assert!(!identity.is_supported());
		assert_eq!(identity.token(), "identity");

		// Lowercased, so two spellings of one coding are one value.
		assert_eq!(Coding::from_token("LZMA"), Coding::from_token("lzma"));
		assert!(Coding::Gzip.is_supported());
	}
}
