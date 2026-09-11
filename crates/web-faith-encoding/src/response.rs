//! Deciding and applying the decoding of a response body.

use std::{io, pin::Pin};

use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use http::header::{ACCEPT_ENCODING, HeaderMap};
use tokio_util::io::{ReaderStream, StreamReader};

use crate::{Coding, ContentEncoding};

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
		accept.merge(value);
		accept
	}

	/// Fold one header line's codings into what is already here, the last mention winning.
	fn merge(&mut self, value: &str) {
		let accept = self;
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

impl From<&HeaderMap> for AcceptEncoding {
	/// Read every `Accept-Encoding` line the request carried.
	///
	/// A request that carried none accepts nothing; [`Default`] is what
	/// [`DEFAULT_ACCEPT_ENCODING`] accepts, for a caller that wants that instead.
	fn from(headers: &HeaderMap) -> Self {
		let mut this = Self {
			codings: Vec::new(),
			star: None,
		};
		for value in headers.get_all(ACCEPT_ENCODING) {
			let Ok(value) = value.to_str() else { continue };
			this.merge(value);
		}
		this
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

/// Decode one layer of a response body, and update its headers to match.
///
/// Takes the outermost coding off `headers` and wraps `body` in the decoder for it, so the two
/// cannot disagree about how far the body has been decoded. A body encoded more than once takes
/// one call per layer.
///
/// Leaves both alone when there is nothing to decode: no coding declared, an outermost coding this
/// crate cannot decode or the request did not accept, or a header that could not be read.
pub fn decode(
	headers: &mut HeaderMap,
	body: Pin<Box<ByteStream>>,
	accept: &AcceptEncoding,
) -> Pin<Box<ByteStream>> {
	match ContentEncoding::peel_one_header(headers, accept) {
		Some(coding) => decode_stream(body, coding),
		None => body,
	}
}

/// Wrap a body byte-stream in a decoder for `coding`.
///
/// Trailers are pulled off the frames before this point, so decoding sees data only. A coding this
/// crate cannot decode leaves the stream as it is; [`can_decode_as`](crate::ContentEncoding::can_decode_as)
/// never returns one.
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
	use http::header::{CONTENT_ENCODING, CONTENT_LENGTH, HeaderMap, HeaderValue};

	use super::*;
	use crate::request::{compress_buffer, compress_stream};

	fn decide(content_encoding: &str, accept: &str) -> Option<Coding> {
		let mut headers = HeaderMap::new();
		headers.insert(
			CONTENT_ENCODING,
			HeaderValue::from_str(content_encoding).unwrap(),
		);
		ContentEncoding::from(&headers).can_decode_as(&AcceptEncoding::from(accept))
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
	fn the_outermost_of_several_codings_is_the_one_decoded() {
		// The last applied is the first to unwind.
		assert_eq!(
			decide("gzip, br", DEFAULT_ACCEPT_ENCODING),
			Some(Coding::Brotli)
		);
		assert_eq!(
			decide("br, gzip", DEFAULT_ACCEPT_ENCODING),
			Some(Coding::Gzip)
		);
		assert_eq!(
			decide("identity, gzip", DEFAULT_ACCEPT_ENCODING),
			Some(Coding::Gzip)
		);

		// An outermost coding this crate cannot decode stops the body being touched at all,
		// whatever sits under it.
		assert_eq!(decide("gzip, identity", DEFAULT_ACCEPT_ENCODING), None);
	}

	#[test]
	fn peeling_a_layer_leaves_the_headers_describing_the_rest() {
		let mut headers = HeaderMap::new();
		headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip, br"));
		headers.insert(CONTENT_LENGTH, HeaderValue::from_static("42"));

		let accept = AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING);
		assert_eq!(
			ContentEncoding::peel_one_header(&mut headers, &accept),
			Some(Coding::Brotli)
		);

		// The body is still gzipped, and the headers say so.
		assert_eq!(headers[CONTENT_ENCODING], "gzip");
		// Its length no longer describes what the caller reads.
		assert!(!headers.contains_key(CONTENT_LENGTH));

		// Peeling the last layer leaves nothing to declare.
		assert_eq!(
			ContentEncoding::peel_one_header(&mut headers, &accept),
			Some(Coding::Gzip)
		);
		assert!(!headers.contains_key(CONTENT_ENCODING));
	}

	#[test]
	fn nothing_to_peel_leaves_the_headers_alone() {
		let mut headers = HeaderMap::new();
		headers.insert(CONTENT_ENCODING, HeaderValue::from_static("identity"));
		headers.insert(CONTENT_LENGTH, HeaderValue::from_static("42"));

		let accept = AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING);
		assert_eq!(
			ContentEncoding::peel_one_header(&mut headers, &accept),
			None
		);

		assert_eq!(headers[CONTENT_ENCODING], "identity");
		assert_eq!(headers[CONTENT_LENGTH], "42");
	}

	#[test]
	fn codings_split_across_header_lines_count_together() {
		// The same list as `gzip, br` on one line, so `br` is the outermost either way.
		let mut headers = HeaderMap::new();
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("br"));
		assert_eq!(
			ContentEncoding::from(&headers)
				.can_decode_as(&AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING)),
			Some(Coding::Brotli)
		);
	}

	#[test]
	fn one_coding_split_across_lines_with_an_empty_line_still_decodes() {
		// An empty line contributes no coding, leaving gzip the only one named.
		let mut headers = HeaderMap::new();
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
		headers.append(CONTENT_ENCODING, HeaderValue::from_static(""));
		assert_eq!(
			ContentEncoding::from(&headers)
				.can_decode_as(&AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING)),
			Some(Coding::Gzip)
		);
	}

	#[test]
	fn a_non_ascii_line_is_delivered_as_received() {
		let mut headers = HeaderMap::new();
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
		headers.append(CONTENT_ENCODING, HeaderValue::from_bytes(b"\xff").unwrap());
		assert_eq!(
			ContentEncoding::from(&headers)
				.can_decode_as(&AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING)),
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
			ContentEncoding::from(&headers)
				.can_decode_as(&AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING)),
			None
		);
	}

	#[test]
	fn the_codings_a_response_declared_are_readable() {
		let mut headers = HeaderMap::new();
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("br, gzip"));
		headers.append(CONTENT_ENCODING, HeaderValue::from_static("identity"));

		// In the order applied, across lines, including one this crate cannot decode.
		assert_eq!(
			ContentEncoding::from(&headers).codings(),
			[
				Coding::Brotli,
				Coding::Gzip,
				Coding::Other("identity".into())
			]
		);

		// More than one coding is the caller's to unwind.
		assert_eq!(
			ContentEncoding::from(&headers).can_decode_as(&AcceptEncoding::default()),
			None
		);

		assert!(ContentEncoding::default().codings().is_empty());
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
