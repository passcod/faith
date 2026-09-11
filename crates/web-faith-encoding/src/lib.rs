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
//! - [`compress_buffer`](request::compress_buffer) and [`compress_stream`](request::compress_stream) apply a coding to a request body.
//! - [`layer_content_encoding`](request::layer_content_encoding) names it in a `Content-Encoding`, alongside anything the caller had
//!   already declared.
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
//! // Otherwise it is named last, being applied on top of what was already there.
//! assert_eq!(layer_content_encoding(Some("br"), Coding::Gzip), "br, gzip");
//! # }
//! ```
//!
//! # Responses
//!
//! - [`AcceptEncoding`](response::AcceptEncoding) is what a request advertised.
//! - [`decision`](response::decision) reads it against a response's headers to say which coding the body should be
//!   decoded under, if any.
//! - [`decode_stream`](response::decode_stream) wraps the body in that decoder.
//! - [`strip_decoded_headers`](response::strip_decoded_headers) removes the headers that described the encoded bytes.
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

#![deny(missing_docs)]
// Lets docs.rs label each item with the feature or platform it needs.
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod request;
pub mod response;

/// A content coding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
	/// [RFC 1952](https://www.rfc-editor.org/rfc/rfc1952).
	Gzip,
	/// In its zlib-wrapped form ([RFC 1950](https://www.rfc-editor.org/rfc/rfc1950)), like browsers.
	Deflate,
	/// [RFC 7932](https://www.rfc-editor.org/rfc/rfc7932).
	Brotli,
	/// [RFC 8878](https://www.rfc-editor.org/rfc/rfc8878).
	Zstd,
}

impl Coding {
	/// Match the `compress` option's value, which names a coding by its wire token.
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

	/// The wire token naming this coding in a `Content-Encoding`.
	pub fn token(self) -> &'static str {
		match self {
			Self::Gzip => "gzip",
			Self::Deflate => "deflate",
			Self::Brotli => "br",
			Self::Zstd => "zstd",
		}
	}

	/// Match a single content-coding token, case-insensitively.
	///
	/// `None` for `identity`, an unknown coding, or one this cannot decode.
	pub fn from_token(token: &str) -> Option<Self> {
		let token = token.trim();
		if token.eq_ignore_ascii_case("gzip") || token.eq_ignore_ascii_case("x-gzip") {
			Some(Self::Gzip)
		} else if token.eq_ignore_ascii_case("deflate") {
			Some(Self::Deflate)
		} else if token.eq_ignore_ascii_case("br") {
			Some(Self::Brotli)
		} else if token.eq_ignore_ascii_case("zstd") {
			Some(Self::Zstd)
		} else {
			None
		}
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
		assert_eq!(Coding::from_token("x-gzip"), Some(Coding::Gzip));
		assert_eq!(Coding::from_option("x-gzip"), None);
		assert_eq!(Coding::from_token("GZIP"), Some(Coding::Gzip));
		assert_eq!(Coding::from_option("GZIP"), None);
		assert_eq!(Coding::from_option(" gzip"), None);
		assert_eq!(Coding::from_option("brotli"), None);
		assert_eq!(Coding::from_option("identity"), None);
		assert_eq!(Coding::from_option(""), None);
	}
}
