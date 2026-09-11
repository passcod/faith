//! Negotiate a coding from a response's headers, decode a body under it, and compress one on the
//! way out.
//!
//! Run with `cargo run -p web-faith-encoding --example codings`.

use bytes::Bytes;
use futures::StreamExt as _;
use http::{HeaderMap, HeaderValue};
use web_faith_encoding::{
	Coding, ContentEncoding,
	request::compress_buffer,
	response::{AcceptEncoding, DEFAULT_ACCEPT_ENCODING, decode_stream},
};

#[tokio::main]
async fn main() {
	let accept = AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING);

	let mut headers = HeaderMap::new();
	headers.insert("content-encoding", HeaderValue::from_static("gzip"));
	headers.insert("content-length", HeaderValue::from_static("42"));
	// Just the header half, so the coding is in hand for the round trip below; `response::decode`
	// does this and the body together.
	let coding = ContentEncoding::peel_one_header(&mut headers, &accept)
		.expect("gzip is in the default Accept-Encoding");
	println!("negotiated: {coding:?}");
	println!("headers after decoding: {:?}", headers.keys().count());

	// Round-trip a body through the coding that was negotiated.
	let original = b"the quick brown fox jumps over the lazy dog".repeat(8);
	let compressed = compress_buffer(&original, coding.clone())
		.await
		.expect("gzip compresses");
	println!(
		"{} bytes in, {} bytes out",
		original.len(),
		compressed.len()
	);

	let stream = futures::stream::once(async move { Ok(Bytes::from(compressed)) });
	let mut decoded = decode_stream(Box::pin(stream), coding);
	let mut round_tripped = Vec::new();
	while let Some(chunk) = decoded.next().await {
		round_tripped.extend_from_slice(&chunk.expect("the body decodes"));
	}
	println!("round-tripped intact: {}", round_tripped == original);

	// Compressing a request on top of a coding the caller already applied declares both, in the
	// order they were applied.
	let mut request = HeaderMap::new();
	request.insert("content-encoding", HeaderValue::from_static("br"));

	let layered = ContentEncoding::from(&request).layer(Coding::Gzip);
	if let Some(value) = layered.to_header_value() {
		request.insert("content-encoding", value);
	}
	println!(
		"Content-Encoding: {}",
		request["content-encoding"].to_str().unwrap()
	);
}
