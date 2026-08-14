//! Content coding: Faith owns the decode decision rather than the HTTP stack
//! underneath, so it can rest on the `Accept-Encoding` of the request in hand.
//! The same codings compress a request body under the `compress` option.
//!
//! spec: ENC

use std::{io, pin::Pin};

use async_compression::tokio::bufread::{
	BrotliDecoder, BrotliEncoder, GzipDecoder, GzipEncoder, ZlibDecoder, ZlibEncoder, ZstdDecoder,
	ZstdEncoder,
};
use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use reqwest::header::{CONTENT_ENCODING, CONTENT_LENGTH, HeaderMap};
use tokio::io::AsyncReadExt;
use tokio_util::io::{ReaderStream, StreamReader};

use crate::body::DynStream;

/// The `Accept-Encoding` Faith advertises when the caller advertises none.
///
/// Matches the value reqwest's decompression stack sent before Faith took over the
/// codings, so the wire is unchanged for the default request.
pub(crate) const DEFAULT_ACCEPT_ENCODING: &str = "zstd,gzip,deflate,br";

/// A content coding Faith can decode. Wire tokens: `gzip`, `deflate`, `br`, `zstd`.
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
	/// Match the `compress` option's value, which names a coding by its wire token.
	///
	/// Unlike [`Self::from_token`], which reads a token off the wire and so takes it as
	/// loosely as HTTP writes it, this matches the four documented tokens exactly: the
	/// option is an API surface, and an unrecognised value is refused rather than
	/// guessed at (spec:ENC#compressing-a-request-body).
	pub(crate) fn from_option(value: &str) -> Option<Self> {
		match value {
			"gzip" => Some(Self::Gzip),
			"deflate" => Some(Self::Deflate),
			"br" => Some(Self::Brotli),
			"zstd" => Some(Self::Zstd),
			_ => None,
		}
	}

	/// The wire token naming this coding in a `Content-Encoding`.
	pub(crate) fn token(self) -> &'static str {
		match self {
			Self::Gzip => "gzip",
			Self::Deflate => "deflate",
			Self::Brotli => "br",
			Self::Zstd => "zstd",
		}
	}

	/// Match a single content-coding token, case-insensitively. `None` for
	/// `identity`, an unknown coding, or a coding Faith cannot decode.
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
/// single coding Faith can decode and the request's `Accept-Encoding` accepted it.
/// A `Content-Encoding` naming more than one coding, an unknown coding, or a coding the
/// request did not accept yields `None`, and the body is delivered as received.
pub(crate) fn decision(headers: &HeaderMap, accept: &AcceptEncoding) -> Option<Coding> {
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

/// A request body stream, as reqwest takes one.
pub(crate) type RequestStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>;

/// Compress a buffered request body, yielding the bytes that go on the wire.
///
/// The whole body is known up front, so it compresses in one pass and its length is the
/// `Content-Length` reqwest derives from it (spec:ENC#what-a-compressed-request-sends).
pub(crate) async fn compress_buffer(input: &[u8], coding: Coding) -> io::Result<Vec<u8>> {
	let mut output = Vec::new();
	match coding {
		Coding::Gzip => GzipEncoder::new(input).read_to_end(&mut output).await?,
		Coding::Deflate => ZlibEncoder::new(input).read_to_end(&mut output).await?,
		Coding::Brotli => BrotliEncoder::new(input).read_to_end(&mut output).await?,
		Coding::Zstd => ZstdEncoder::new(input).read_to_end(&mut output).await?,
	};
	Ok(output)
}

/// Compress a streaming request body as its chunks arrive.
///
/// There is no compressed length to declare before the body ends, so the result goes out
/// chunked (spec:ENC#what-a-compressed-request-sends). The encoder buffers on its own
/// terms, so the bytes for one chunk the caller writes need not leave with it.
pub(crate) fn compress_stream<S>(input: S, coding: Coding) -> RequestStream
where
	S: Stream<Item = io::Result<Bytes>> + Send + 'static,
{
	let reader = StreamReader::new(input);
	match coding {
		Coding::Gzip => encoder_stream(GzipEncoder::new(reader)),
		Coding::Deflate => encoder_stream(ZlibEncoder::new(reader)),
		Coding::Brotli => encoder_stream(BrotliEncoder::new(reader)),
		Coding::Zstd => encoder_stream(ZstdEncoder::new(reader)),
	}
}

fn encoder_stream<R>(reader: R) -> RequestStream
where
	R: tokio::io::AsyncRead + Send + 'static,
{
	Box::pin(ReaderStream::new(reader))
}

/// Join the codings a request already declares with the one Faith applied.
///
/// The caller's `Content-Encoding` describes the bytes they handed over, so Faith's coding
/// is named after theirs, the order the codings were applied in
/// (spec:ENC#what-a-compressed-request-sends).
pub(crate) fn layer_content_encoding(declared: Option<&str>, applied: Coding) -> String {
	match declared.map(str::trim).filter(|value| !value.is_empty()) {
		Some(declared) => format!("{declared}, {}", applied.token()),
		None => applied.token().to_owned(),
	}
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

	#[tokio::test]
	async fn a_compressed_body_decodes_back_to_what_went_in() {
		// The bytes a server receives are the bytes the caller supplied, whichever coding
		// carried them: Faith's own decoder is the check.
		let input = b"the quick brown fox jumps over the lazy dog".repeat(20);
		for coding in [Coding::Gzip, Coding::Deflate, Coding::Brotli, Coding::Zstd] {
			let compressed = compress_buffer(&input, coding).await.unwrap();
			assert!(
				compressed.len() < input.len(),
				"{coding:?} did not compress repetitive input"
			);

			let source = futures::stream::once(async move { Ok(Bytes::from(compressed)) });
			let decoded: Vec<u8> = decode_stream(Box::pin(source), coding)
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

	#[test]
	fn faiths_coding_is_named_after_the_codings_the_caller_declared() {
		assert_eq!(
			layer_content_encoding(Some("gzip"), Coding::Zstd),
			"gzip, zstd"
		);
		assert_eq!(
			layer_content_encoding(Some("gzip, br"), Coding::Deflate),
			"gzip, br, deflate"
		);
	}

	#[test]
	fn a_request_declaring_nothing_names_only_the_coding_faith_applied() {
		assert_eq!(layer_content_encoding(None, Coding::Brotli), "br");
		assert_eq!(layer_content_encoding(Some(""), Coding::Gzip), "gzip");
		assert_eq!(layer_content_encoding(Some("  "), Coding::Gzip), "gzip");
	}
}
