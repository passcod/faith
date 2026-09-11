//! Applying a content coding to a request body.

use std::{io, pin::Pin};

use async_compression::tokio::bufread::{BrotliEncoder, GzipEncoder, ZlibEncoder, ZstdEncoder};
use bytes::Bytes;
use futures::Stream;
use tokio::io::AsyncReadExt;
use tokio_util::io::{ReaderStream, StreamReader};

use crate::Coding;

/// A request body stream, as reqwest takes one.
pub type RequestStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>;

/// Compress a buffered request body, yielding the bytes that go on the wire.
///
/// The length of the result is the `Content-Length` the request can declare.
// spec:ENC#what-a-compressed-request-sends
pub async fn compress_buffer(input: &[u8], coding: Coding) -> io::Result<Vec<u8>> {
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
/// chunked. The encoder buffers on its own terms, so the bytes for one chunk the caller
/// writes need not leave with it.
// spec:ENC#what-a-compressed-request-sends
pub fn compress_stream<S>(input: S, coding: Coding) -> RequestStream
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

/// Join the codings a request already declares with the one applied on top.
///
/// The caller's `Content-Encoding` describes the bytes they handed over, so the applied coding is
/// named after theirs: the order the codings were applied in.
// spec:ENC#what-a-compressed-request-sends
pub fn layer_content_encoding(declared: Option<&str>, applied: Coding) -> String {
	match declared.map(str::trim).filter(|value| !value.is_empty()) {
		Some(declared) => format!("{declared}, {}", applied.token()),
		None => applied.token().to_owned(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
