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

use http::header::{CONTENT_ENCODING, CONTENT_LENGTH, HeaderMap};

use crate::{Coding, ContentEncoding};

/// A request body stream, as reqwest takes one.
pub type RequestStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>;

/// Compress a request body in `coding`, and declare it in the headers.
///
/// Returns the bytes that go on the wire, with `Content-Encoding` naming `coding` after whatever
/// the caller had already declared, and `Content-Length` removed: it described the body before
/// compression. The two cannot disagree about what the body carries.
///
/// [`compress_buffer`] and [`ContentEncoding::layer`] are the halves, for a caller driving them
/// separately.
pub async fn encode(headers: &mut HeaderMap, body: &[u8], coding: Coding) -> io::Result<Vec<u8>> {
	let compressed = compress_buffer(body, coding.clone()).await?;
	declare(headers, coding)?;
	Ok(compressed)
}

/// Compress a streaming request body in `coding`, and declare it in the headers.
///
/// As [`encode`], for a body arriving in chunks. It goes out chunked, having no length to declare.
pub fn encode_stream<S>(
	headers: &mut HeaderMap,
	body: S,
	coding: Coding,
) -> io::Result<RequestStream>
where
	S: Stream<Item = io::Result<Bytes>> + Send + 'static,
{
	let compressed = compress_stream(body, coding.clone())?;
	declare(headers, coding)?;
	Ok(compressed)
}

/// Add `coding` to what the headers already declare, and drop the length it no longer describes.
fn declare(headers: &mut HeaderMap, coding: Coding) -> io::Result<()> {
	let layered = ContentEncoding::from(&*headers).layer(coding);
	let value = layered
		.to_header_value()
		.ok_or_else(|| io::Error::other(format!("cannot declare {layered:?}")))?;

	headers.insert(CONTENT_ENCODING, value);
	headers.remove(CONTENT_LENGTH);
	Ok(())
}

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
