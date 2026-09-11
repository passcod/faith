//! Applying a content coding to a request body.
//!
//! There is no standard signal that indicates a server accepts request body encoding ahead of
//! sending. Therefore, doing so always requires out-of-band knowledge in some way or shape.
//! ([RFC 9110 §15.5.16](https://www.rfc-editor.org/rfc/rfc9110#section-15.5.16) does specify that
//! servers should answer encodings they can't decode with an `Accept-Encoding` header; that would
//! require buffering and re-sending the request, so we don't implement it automatically.)

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
		other => return Err(unsupported(&other)),
	};
	Ok(output)
}

/// The error a coding this crate cannot apply produces.
fn unsupported(coding: &Coding) -> io::Error {
	io::Error::new(
		io::ErrorKind::Unsupported,
		format!("cannot compress in {:?}", coding.token()),
	)
}

/// Compress a streaming request body as its chunks arrive.
///
/// There is no compressed length to declare before the body ends, so the result goes out
/// chunked. The encoder buffers on its own terms, so the bytes for one chunk the caller
/// writes need not leave with it.
// spec:ENC#what-a-compressed-request-sends
pub fn compress_stream<S>(input: S, coding: Coding) -> io::Result<RequestStream>
where
	S: Stream<Item = io::Result<Bytes>> + Send + 'static,
{
	let reader = StreamReader::new(input);
	Ok(match coding {
		Coding::Gzip => encoder_stream(GzipEncoder::new(reader)),
		Coding::Deflate => encoder_stream(ZlibEncoder::new(reader)),
		Coding::Brotli => encoder_stream(BrotliEncoder::new(reader)),
		Coding::Zstd => encoder_stream(ZstdEncoder::new(reader)),
		other => return Err(unsupported(&other)),
	})
}

fn encoder_stream<R>(reader: R) -> RequestStream
where
	R: tokio::io::AsyncRead + Send + 'static,
{
	Box::pin(ReaderStream::new(reader))
}
