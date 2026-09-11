//! Deciding and applying the decoding of a response body.

use std::{io, pin::Pin};

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use http::header::{CONTENT_ENCODING, CONTENT_LENGTH, HeaderMap};
use tokio_util::io::{ReaderStream, StreamReader};

use crate::Coding;

/// A body byte-stream, as the decoders take and return one.
pub type ByteStream = dyn Stream<Item = Result<Bytes, String>> + Send + Sync;

/// The `Accept-Encoding` Faith advertises when the caller advertises none.
pub const DEFAULT_ACCEPT_ENCODING: &str = "zstd,gzip,deflate,br";

/// What a request's `Accept-Encoding` accepts.
///
/// Only the codings in [`Coding`] and `*` are kept; any other token is ignored, since a coding
/// this crate cannot decode is not a coding it can choose. Parsing does not fail: a malformed
/// quality value reads as `q=0`, which refuses that coding.
///
/// [`Default`] is what [`DEFAULT_ACCEPT_ENCODING`] accepts, not the empty set.
#[derive(Clone, Debug)]
pub struct AcceptEncoding {
	/// Every coding named outright, in the order the header named them.
	codings: Vec<(Coding, u16)>,
	/// The quality `*` was given, if it was named.
	star: Option<u16>,
}

impl AcceptEncoding {
	fn parse(value: &str) -> Self {
		let mut accept = Self {
			codings: Vec::new(),
			star: None,
		};
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

			if token == "*" {
				accept.star = Some(quality);
				continue;
			}

			let coding = Coding::from_token(token);
			// A header may name a coding twice; the last wins, as the last of any repeated
			// header field value does.
			match accept
				.codings
				.iter_mut()
				.find(|(named, _)| *named == coding)
			{
				Some((_, existing)) => *existing = quality,
				None => accept.codings.push((coding, quality)),
			}
		}
		accept
	}

	/// Every coding the header named outright, with the quality value it carried.
	///
	/// In the order the header named them, and including codings this crate cannot decode. A
	/// coding named with `q=0` is present here and refused by [`Self::accepts`].
	pub fn iter(&self) -> impl Iterator<Item = (&Coding, u16)> {
		self.codings
			.iter()
			.map(|(coding, quality)| (coding, *quality))
	}

	/// The quality value `coding` was named with, or `None` if the header did not name it.
	///
	/// Does not consult `*`; see [`Self::star`].
	pub fn quality(&self, coding: &Coding) -> Option<u16> {
		self.codings
			.iter()
			.find(|(named, _)| named == coding)
			.map(|(_, quality)| *quality)
	}

	/// The quality value `*` was named with, if the header named it.
	pub fn star(&self) -> Option<u16> {
		self.star
	}

	/// Whether a coding was accepted.
	///
	/// A coding named outright settles it whatever `*` says, so a zero quality value on the named
	/// coding refuses it even where `*` would accept.
	/// Whether `coding` may be used for the response body.
	pub fn accepts(&self, coding: &Coding) -> bool {
		match self.quality(coding) {
			Some(quality) => quality > 0,
			None => matches!(self.star, Some(quality) if quality > 0),
		}
	}
}

impl From<&str> for AcceptEncoding {
	fn from(value: &str) -> Self {
		Self::parse(value)
	}
}

impl Default for AcceptEncoding {
	fn default() -> Self {
		Self::parse(DEFAULT_ACCEPT_ENCODING)
	}
}

/// Decide whether and how to decode a response body.
///
/// The coding to decode under when the response's `Content-Encoding` carries a single coding that
/// can be decoded and the request's `Accept-Encoding` accepted it. Otherwise `None`, and the body
/// is delivered as received.
pub fn decision(headers: &HeaderMap, accept: &AcceptEncoding) -> Option<Coding> {
	// A representation encoded more than once is the caller's to unwind. The codings may
	// arrive comma-joined on one line or split across several `Content-Encoding` lines --
	// the same list either way, so both forms are gathered together before counting.
	let mut codings = Vec::new();
	for value in headers.get_all(CONTENT_ENCODING) {
		// A line that is not valid ASCII names nothing Faith can match; deliver as received
		// rather than decoding whatever line sits beside it.
		let value = value.to_str().ok()?;
		codings.extend(value.split(',').map(str::trim).filter(|c| !c.is_empty()));
	}

	let [single] = codings[..] else {
		return None;
	};
	// `identity` and anything unknown land in `Other`, which is nothing to decode under.
	let coding = Coding::from_token(single);
	(coding.is_supported() && accept.accepts(&coding)).then_some(coding)
}

/// Strip the headers that describe the encoded bytes, once a body has been decoded.
pub fn strip_decoded_headers(headers: &mut HeaderMap) {
	headers.remove(CONTENT_ENCODING);
	headers.remove(CONTENT_LENGTH);
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
/// Trailers are pulled off the frames before this point, so decoding sees data only. A coding this
/// crate cannot decode leaves the stream as it is; [`decision`] never returns one.
pub fn decode_stream(input: Pin<Box<ByteStream>>, coding: Coding) -> Pin<Box<ByteStream>> {
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
		_ => reader_stream(reader),
	}
}

fn reader_stream<R>(reader: R) -> Pin<Box<ByteStream>>
where
	R: tokio::io::AsyncRead + Send + Sync + 'static,
{
	Box::pin(ReaderStream::new(reader).map_err(|err| err.to_string()))
}

#[cfg(test)]
mod tests {
	use http::header::{CONTENT_ENCODING, HeaderMap, HeaderValue};

	use super::*;
	use crate::request::{compress_buffer, compress_stream};

	fn decide(content_encoding: &str, accept: &str) -> Option<Coding> {
		let mut headers = HeaderMap::new();
		headers.insert(
			CONTENT_ENCODING,
			HeaderValue::from_str(content_encoding).unwrap(),
		);
		decision(&headers, &AcceptEncoding::from(accept))
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
		assert_eq!(decide("identity, gzip", DEFAULT_ACCEPT_ENCODING), None);
	}

	#[test]
	fn codings_split_across_header_lines_count_together() {
		// The same list as `gzip, br` on one line, so neither coding is decoded.
		let mut headers = HeaderMap::new();
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("br"));
		assert_eq!(
			decision(&headers, &AcceptEncoding::parse(DEFAULT_ACCEPT_ENCODING)),
			None
		);
	}

	#[test]
	fn one_coding_split_across_lines_with_an_empty_line_still_decodes() {
		// An empty line contributes no coding, leaving gzip the only one named.
		let mut headers = HeaderMap::new();
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
		headers.append(CONTENT_ENCODING, HeaderValue::from_static(""));
		assert_eq!(
			decision(&headers, &AcceptEncoding::parse(DEFAULT_ACCEPT_ENCODING)),
			Some(Coding::Gzip)
		);
	}

	#[test]
	fn a_non_ascii_line_is_delivered_as_received() {
		let mut headers = HeaderMap::new();
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
		headers.append(CONTENT_ENCODING, HeaderValue::from_bytes(b"\xff").unwrap());
		assert_eq!(
			decision(&headers, &AcceptEncoding::parse(DEFAULT_ACCEPT_ENCODING)),
			None
		);
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

	#[tokio::test]
	async fn a_compressed_body_decodes_back_to_what_went_in() {
		// The bytes a server receives are the bytes the caller supplied, whichever coding
		// carried them: Faith's own decoder is the check.
		let input = b"the quick brown fox jumps over the lazy dog".repeat(20);
		for coding in [Coding::Gzip, Coding::Deflate, Coding::Brotli, Coding::Zstd] {
			let compressed = compress_buffer(&input, coding.clone()).await.unwrap();
			assert!(
				compressed.len() < input.len(),
				"{coding:?} did not compress repetitive input"
			);

			let source = futures::stream::once(async move { Ok(Bytes::from(compressed)) });
			let decoded: Vec<u8> = decode_stream(Box::pin(source), coding.clone())
				.try_fold(Vec::new(), |mut acc, chunk| async move {
					acc.extend_from_slice(&chunk);
					Ok(acc)
				})
				.await
				.unwrap();
			assert_eq!(decoded, input, "{coding:?} round trip");
		}
	}

	#[tokio::test]
	async fn a_streaming_body_compresses_across_its_chunks() {
		let chunks = ["first chunk, ", "second chunk, ", "third chunk"];
		let source = futures::stream::iter(
			chunks
				.into_iter()
				.map(|chunk| Ok(Bytes::from_static(chunk.as_bytes()))),
		);

		let compressed: Vec<u8> = compress_stream(source, Coding::Zstd)
			.expect("zstd compresses")
			.try_fold(Vec::new(), |mut acc, chunk| async move {
				acc.extend_from_slice(&chunk);
				Ok(acc)
			})
			.await
			.unwrap();

		let source = futures::stream::once(async move { Ok(Bytes::from(compressed)) });
		let decoded: Vec<u8> = decode_stream(Box::pin(source), Coding::Zstd)
			.try_fold(Vec::new(), |mut acc, chunk| async move {
				acc.extend_from_slice(&chunk);
				Ok(acc)
			})
			.await
			.unwrap();
		assert_eq!(decoded, chunks.concat().as_bytes());
	}
}
