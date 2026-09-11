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
//! - Use [`ContentEncoding::layer`] to add it to whatever the caller already declared, and
//!   [`to_header_value`](ContentEncoding::to_header_value) to build the header.
//!
//! ```
//! use http::{HeaderMap, HeaderValue};
//! use web_faith_encoding::{Coding, ContentEncoding, request::compress_buffer};
//!
//! # async fn example() {
//! let body = b"the quick brown fox".repeat(8);
//! let compressed = compress_buffer(&body, Coding::Gzip).await.expect("gzip compresses");
//! assert!(compressed.len() < body.len());
//!
//! let mut headers = HeaderMap::new();
//! headers.insert("content-encoding", HeaderValue::from_static("br"));
//!
//! // Applied last, so declared last.
//! let layered = ContentEncoding::from(&headers).layer(Coding::Gzip);
//! headers.insert("content-encoding", layered.to_header_value().expect("two codings"));
//! assert_eq!(headers["content-encoding"], "br, gzip");
//! # }
//! ```
//!
//! # Responses
//!
//! - Use [`AcceptEncoding`] to parse the advertised supported coding set from the request.
//! - Use [`decode`] to take one layer off a response: it decodes the body and updates the headers
//!   together, so the two cannot disagree about how far it has been decoded.
//! - A body encoded more than once takes one call per layer.
//!
//! To drive the halves separately, [`ContentEncoding::peel_one_header`] does the headers and
//! [`decode_stream`] does the body.
//!
//! ```
//! use http::{HeaderMap, HeaderValue};
//! use web_faith_encoding::{
//!     Coding,
//!     ContentEncoding,
//!     response::AcceptEncoding,
//! };
//!
//! let mut request = HeaderMap::new();
//! request.insert("accept-encoding", HeaderValue::from_static("gzip, br;q=0.5"));
//! let accept = AcceptEncoding::from(&request);
//!
//! let mut response = HeaderMap::new();
//! response.insert("content-encoding", HeaderValue::from_static("br, gzip"));
//! response.insert("content-length", HeaderValue::from_static("42"));
//!
//! // The outermost layer, and the headers left describing what is still encoded under it.
//! assert_eq!(ContentEncoding::peel_one_header(&mut response, &accept), Some(Coding::Gzip));
//! assert_eq!(response["content-encoding"], "br");
//! assert!(!response.contains_key("content-length"));
//! ```
//!
//! [`compress_buffer`]: request::compress_buffer
//! [`compress_stream`]: request::compress_stream
//! [`AcceptEncoding`]: response::AcceptEncoding
//! [`decode`]: response::decode
//! [`decode_stream`]: response::decode_stream

#![deny(missing_docs)]
// Lets docs.rs label each item with the feature or platform it needs.
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod request;
pub mod response;

use std::fmt;

use http::header::{CONTENT_ENCODING, CONTENT_LENGTH, HeaderMap, HeaderValue};

use crate::response::AcceptEncoding;

/// The codings a response says its body carries.
///
/// Read from every `Content-Encoding` line together: a representation encoded more than once may
/// arrive comma-joined on one line or split across several, and it is the same list either way.
#[derive(Clone, Debug, Default)]
pub struct ContentEncoding {
	codings: Vec<Coding>,
	/// A line that was not valid ASCII, so what the body carries is not knowable.
	unreadable: bool,
}

impl From<&str> for ContentEncoding {
	/// Read one `Content-Encoding` header value.
	fn from(value: &str) -> Self {
		let mut this = Self::default();
		this.merge(value);
		this
	}
}

impl From<&HeaderMap> for ContentEncoding {
	fn from(headers: &HeaderMap) -> Self {
		let mut this = Self::default();
		for value in headers.get_all(CONTENT_ENCODING) {
			let Ok(value) = value.to_str() else {
				this.unreadable = true;
				continue;
			};
			this.merge(value);
		}
		this
	}
}

impl ContentEncoding {
	/// Fold one header value's codings into what is already here.
	fn merge(&mut self, value: &str) {
		self.codings.extend(
			value
				.split(',')
				.map(str::trim)
				.filter(|token| !token.is_empty())
				.map(Coding::from_token),
		);
	}

	/// The codings the header carried, in the order it applied them.
	///
	/// Empty for a response that declared none, and for one whose header could not be read.
	pub fn codings(&self) -> &[Coding] {
		&self.codings
	}

	/// Add a coding on top of the ones already here.
	///
	/// Applied last, so it is last in the header: the codings are listed in the order they were
	/// applied, and a reader unwinds them in reverse.
	pub fn layer(&self, coding: Coding) -> Self {
		let mut layered = self.clone();
		layered.codings.push(coding);
		layered
	}

	/// The header value these codings make, or `None` when there are none to declare.
	pub fn to_header_value(&self) -> Option<HeaderValue> {
		if self.codings.is_empty() {
			return None;
		}

		HeaderValue::from_str(&self.to_string()).ok()
	}

	/// The coding the next layer of the body is under, given what the request accepted.
	///
	/// The last coding, being the last applied and so the first to unwind. `None` leaves the body
	/// as it arrived, which covers a response that declared no coding, one whose outermost coding
	/// this crate cannot decode (`identity` among them) or the request did not accept, and one
	/// whose header was not readable.
	pub fn can_decode_as(&self, accept: &AcceptEncoding) -> Option<Coding> {
		if self.unreadable {
			return None;
		}

		let outermost = self.codings.last()?;
		(outermost.is_supported() && accept.accepts(outermost)).then(|| outermost.clone())
	}

	/// These codings with the outermost removed, as the body stands once it is decoded.
	pub fn peeled(&self) -> Self {
		let mut peeled = self.clone();
		peeled.codings.pop();
		peeled
	}

	/// Take one layer off `headers`, returning the coding its body is under.
	///
	/// The headers are left describing the body once that coding has been decoded, which
	/// [`response::decode`] does in the same call. Reach for this only to
	/// drive the two halves separately.
	///
	/// `Content-Encoding` keeps whatever layers remain and goes when none do; `Content-Length`
	/// goes either way, no longer describing what the caller reads. `None` leaves `headers` as
	/// they are.
	pub fn peel_one_header(headers: &mut HeaderMap, accept: &AcceptEncoding) -> Option<Coding> {
		let encoding = Self::from(&*headers);
		let coding = encoding.can_decode_as(accept)?;

		match encoding.peeled().to_header_value() {
			Some(value) => headers.insert(CONTENT_ENCODING, value),
			None => headers.remove(CONTENT_ENCODING),
		};
		headers.remove(CONTENT_LENGTH);

		Some(coding)
	}
}

impl fmt::Display for ContentEncoding {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		for (n, coding) in self.codings.iter().enumerate() {
			if n > 0 {
				f.write_str(", ")?;
			}
			f.write_str(coding.token())?;
		}
		Ok(())
	}
}

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

	use http::header::HeaderValue;

	fn headers(value: &str) -> HeaderMap {
		let mut headers = HeaderMap::new();
		headers.insert(CONTENT_ENCODING, HeaderValue::from_str(value).unwrap());
		headers
	}

	#[test]
	fn a_layered_coding_is_added_last() {
		let existing = ContentEncoding::from(&headers("gzip"));
		let layered = existing.layer(Coding::Zstd);
		assert_eq!(layered.to_string(), "gzip, zstd");
		assert_eq!(
			ContentEncoding::from(&headers("gzip, br"))
				.layer(Coding::Deflate)
				.to_string(),
			"gzip, br, deflate"
		);

		// Layering leaves what it was called on alone.
		assert_eq!(existing.to_string(), "gzip");
	}

	#[test]
	fn a_request_declaring_nothing_carries_only_the_layered_coding() {
		assert_eq!(
			ContentEncoding::default().layer(Coding::Brotli).to_string(),
			"br"
		);
		// An empty or blank header declares nothing.
		for value in ["", "  "] {
			assert_eq!(
				ContentEncoding::from(&headers(value))
					.layer(Coding::Gzip)
					.to_string(),
				"gzip"
			);
		}
	}

	#[test]
	fn a_coding_this_crate_cannot_apply_is_still_declarable() {
		// The caller compressed in a coding of their own and declared it; Faith layers gzip over
		// the top, and the header names both in the order they were applied.
		let layered = ContentEncoding::from(&headers("custom-thing")).layer(Coding::Gzip);
		assert_eq!(layered.to_string(), "custom-thing, gzip");
		assert_eq!(
			layered.codings(),
			[Coding::Other("custom-thing".into()), Coding::Gzip]
		);

		// Applying it is another matter: this crate has no encoder for it.
		assert!(
			futures::executor::block_on(crate::request::compress_buffer(
				b"x",
				Coding::Other("custom-thing".into())
			))
			.is_err()
		);
	}

	#[test]
	fn nothing_to_declare_makes_no_header() {
		assert!(ContentEncoding::default().to_header_value().is_none());
		assert_eq!(
			ContentEncoding::default()
				.layer(Coding::Gzip)
				.to_header_value()
				.unwrap(),
			"gzip"
		);
	}

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
