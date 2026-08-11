//! Content coding: Fáith owns the decode decision rather than the HTTP stack
//! underneath, so it can rest on the `Accept-Encoding` of the request in hand.
//!
//! spec: ENC

use std::{io, pin::Pin};

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
use futures::TryStreamExt;
use reqwest::header::{CONTENT_ENCODING, CONTENT_LENGTH, HeaderMap};
use tokio_util::io::{ReaderStream, StreamReader};

use crate::body::DynStream;

/// The `Accept-Encoding` Fáith advertises when the caller advertises none.
///
/// Matches the value reqwest's decompression stack sent before Fáith took over the
/// codings, so the wire is unchanged for the default request.
pub(crate) const DEFAULT_ACCEPT_ENCODING: &str = "zstd,gzip,deflate,br";

/// A content coding Fáith can decode. Wire tokens: `gzip`, `deflate`, `br`, `zstd`.
///
/// `deflate` is the zlib-wrapped form (RFC 1950), matching what reqwest and every
/// other mainstream client decode it as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coding {
	Gzip,
	Deflate,
	Brotli,
	Zstd,
}

impl Coding {
	/// Match a single content-coding token, case-insensitively. `None` for
	/// `identity`, an unknown coding, or a coding Fáith cannot decode.
	fn from_token(token: &str) -> Option<Self> {
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

/// Decide whether and how to decode a response body.
///
/// Returns the coding to decode under when the response's `Content-Encoding` names a
/// single coding Fáith can decode and the request's `Accept-Encoding` accepted it.
/// A `Content-Encoding` naming more than one coding, an unknown coding, or a coding the
/// request did not accept yields `None`, and the body is delivered as received.
pub(crate) fn decision(headers: &HeaderMap, accept: &AcceptEncoding) -> Option<Coding> {
	let value = headers.get(CONTENT_ENCODING)?.to_str().ok()?;

	// A representation encoded more than once is the caller's to unwind.
	let mut codings = value.split(',').filter(|token| !token.trim().is_empty());
	let single = codings.next()?;
	if codings.next().is_some() {
		return None;
	}

	let coding = Coding::from_token(single)?;
	accept.accepts(coding).then_some(coding)
}

/// Strip the headers that describe the encoded bytes, once a body has been decoded.
pub(crate) fn strip_decoded_headers(headers: &mut HeaderMap) {
	headers.remove(CONTENT_ENCODING);
	headers.remove(CONTENT_LENGTH);
}

/// A parsed `Accept-Encoding`, enough to answer whether a coding was accepted.
///
/// Each slot holds the quality value (0..=1000) a coding was named with, if it was
/// named outright; `star` holds the quality value of `*` if present.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AcceptEncoding {
	gzip: Option<u16>,
	deflate: Option<u16>,
	brotli: Option<u16>,
	zstd: Option<u16>,
	star: Option<u16>,
}

impl AcceptEncoding {
	pub(crate) fn parse(value: &str) -> Self {
		let mut accept = Self::default();
		for element in value.split(',') {
			let mut parts = element.split(';');
			let Some(token) = parts.next().map(str::trim) else {
				continue;
			};
			if token.is_empty() {
				continue;
			}

			let mut quality = 1000;
			for param in parts {
				let param = param.trim();
				if let Some(rest) = param
					.strip_prefix("q=")
					.or_else(|| param.strip_prefix("Q="))
				{
					quality = parse_quality(rest).unwrap_or(0);
				}
			}

			let slot = if token == "*" {
				&mut accept.star
			} else {
				match Coding::from_token(token) {
					Some(Coding::Gzip) => &mut accept.gzip,
					Some(Coding::Deflate) => &mut accept.deflate,
					Some(Coding::Brotli) => &mut accept.brotli,
					Some(Coding::Zstd) => &mut accept.zstd,
					None => continue,
				}
			};
			*slot = Some(quality);
		}
		accept
	}

	/// Whether a coding was accepted: a coding named outright settles the question
	/// whatever `*` says, so a zero quality value on the named coding refuses it even
	/// when `*` would accept.
	fn accepts(&self, coding: Coding) -> bool {
		let named = match coding {
			Coding::Gzip => self.gzip,
			Coding::Deflate => self.deflate,
			Coding::Brotli => self.brotli,
			Coding::Zstd => self.zstd,
		};
		match named {
			Some(quality) => quality > 0,
			None => matches!(self.star, Some(quality) if quality > 0),
		}
	}
}

/// Parse an RFC 9110 quality value into thousandths (so `0.5` is `500`).
fn parse_quality(value: &str) -> Option<u16> {
	let value = value.trim();
	let mut chars = value.chars();
	let mut quality: u16 = match chars.next()? {
		'0' => 0,
		'1' => 1000,
		_ => return None,
	};
	if let Some(dot) = chars.next() {
		if dot != '.' {
			return None;
		}
		let mut scale = 100;
		for digit in chars {
			quality += digit.to_digit(10)? as u16 * scale;
			if scale == 1 {
				break;
			}
			scale /= 10;
		}
	}
	Some(quality.min(1000))
}

/// Wrap a body byte-stream in a decoder for `coding`.
///
/// The body stream carries decoded bytes on every read path this way (see [`Coding`]);
/// trailers are pulled off the frames before this point, so decoding sees data only.
pub(crate) fn decode_stream(input: Pin<Box<DynStream>>, coding: Coding) -> Pin<Box<DynStream>> {
	let reader = StreamReader::new(input.map_err(io::Error::other));
	match coding {
		Coding::Gzip => reader_stream(GzipDecoder::new(reader)),
		Coding::Deflate => reader_stream(ZlibDecoder::new(reader)),
		Coding::Brotli => reader_stream(BrotliDecoder::new(reader)),
		Coding::Zstd => {
			// A zstd body can be several concatenated frames, as reqwest's stack decoded it.
			let mut decoder = ZstdDecoder::new(reader);
			decoder.multiple_members(true);
			reader_stream(decoder)
		}
	}
}

fn reader_stream<R>(reader: R) -> Pin<Box<DynStream>>
where
	R: tokio::io::AsyncRead + Send + Sync + 'static,
{
	Box::pin(ReaderStream::new(reader).map_err(|err| err.to_string()))
}

#[cfg(test)]
mod tests {
	use reqwest::header::{CONTENT_ENCODING, HeaderMap, HeaderValue};

	use super::*;

	fn decide(content_encoding: &str, accept: &str) -> Option<Coding> {
		let mut headers = HeaderMap::new();
		headers.insert(
			CONTENT_ENCODING,
			HeaderValue::from_str(content_encoding).unwrap(),
		);
		decision(&headers, &AcceptEncoding::parse(accept))
	}

	#[test]
	fn decodes_a_negotiated_coding() {
		assert_eq!(decide("gzip", DEFAULT_ACCEPT_ENCODING), Some(Coding::Gzip));
		assert_eq!(decide("br", DEFAULT_ACCEPT_ENCODING), Some(Coding::Brotli));
		assert_eq!(decide("zstd", DEFAULT_ACCEPT_ENCODING), Some(Coding::Zstd));
		assert_eq!(
			decide("deflate", DEFAULT_ACCEPT_ENCODING),
			Some(Coding::Deflate)
		);
	}

	#[test]
	fn a_coding_named_alone_decodes_only_itself() {
		assert_eq!(decide("gzip", "gzip"), Some(Coding::Gzip));
		assert_eq!(decide("br", "gzip"), None);
	}

	#[test]
	fn identity_leaves_a_compressed_body_alone() {
		assert_eq!(decide("gzip", "identity"), None);
	}

	#[test]
	fn a_zero_quality_value_refuses() {
		assert_eq!(decide("gzip", "gzip;q=0"), None);
		assert_eq!(decide("gzip", "gzip;q=0.000"), None);
	}

	#[test]
	fn a_named_coding_settles_the_question_over_star() {
		// `gzip;q=0, *` refuses gzip while accepting the other three.
		assert_eq!(decide("gzip", "gzip;q=0, *"), None);
		assert_eq!(decide("br", "gzip;q=0, *"), Some(Coding::Brotli));
		assert_eq!(decide("zstd", "gzip;q=0, *"), Some(Coding::Zstd));
	}

	#[test]
	fn star_covers_what_is_not_named() {
		assert_eq!(decide("gzip", "*"), Some(Coding::Gzip));
		assert_eq!(decide("gzip", "br, *"), Some(Coding::Gzip));
	}

	#[test]
	fn a_star_with_zero_quality_accepts_nothing_unnamed() {
		assert_eq!(decide("gzip", "*;q=0"), None);
		assert_eq!(decide("gzip", "gzip, *;q=0"), Some(Coding::Gzip));
	}

	#[test]
	fn more_than_one_coding_is_delivered_as_received() {
		assert_eq!(decide("gzip, br", DEFAULT_ACCEPT_ENCODING), None);
		assert_eq!(decide("br, gzip", DEFAULT_ACCEPT_ENCODING), None);
	}

	#[test]
	fn a_coding_faith_cannot_decode_is_delivered_as_received() {
		assert_eq!(decide("compress", DEFAULT_ACCEPT_ENCODING), None);
	}

	#[test]
	fn no_content_encoding_means_nothing_to_decode() {
		let headers = HeaderMap::new();
		assert_eq!(
			decision(&headers, &AcceptEncoding::parse(DEFAULT_ACCEPT_ENCODING)),
			None
		);
	}

	#[test]
	fn quality_values_parse_to_thousandths() {
		assert_eq!(parse_quality("0"), Some(0));
		assert_eq!(parse_quality("1"), Some(1000));
		assert_eq!(parse_quality("0.5"), Some(500));
		assert_eq!(parse_quality("0.001"), Some(1));
		assert_eq!(parse_quality("1.0"), Some(1000));
	}
}
