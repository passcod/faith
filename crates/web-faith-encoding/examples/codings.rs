//! Negotiate a coding from a response's headers, decode a body under it, and compress one on the
//! way out.
//!
//! Run with `cargo run -p web-faith-encoding --example codings`.

use bytes::Bytes;
use futures::StreamExt as _;
use http::{HeaderMap, HeaderValue};
use web_faith_encoding::{
	Coding,
	request::{compress_buffer, layer_content_encoding},
	response::{
		AcceptEncoding, DEFAULT_ACCEPT_ENCODING, decision, decode_stream, strip_decoded_headers,
	},
};

#[tokio::main]
async fn main() {
	let accept = AcceptEncoding::from(DEFAULT_ACCEPT_ENCODING);

	// What a response's headers negotiate against what the request asked for.
	let mut headers = HeaderMap::new();
	headers.insert("content-encoding", HeaderValue::from_static("gzip"));
	headers.insert("content-length", HeaderValue::from_static("42"));
	let coding = decision(&headers, &accept).expect("gzip is in the default Accept-Encoding");
	println!("negotiated: {coding:?}");

	// A decoded body's length and coding no longer describe what the caller receives.
	strip_decoded_headers(&mut headers);
	println!("headers after decoding: {:?}", headers.keys().count());

	// Round-trip a body through the coding that was negotiated.
	let original = b"the quick brown fox jumps over the lazy dog".repeat(8);
	let compressed = compress_buffer(&original, coding)
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

	// A request that compresses on top of a coding the caller already applied names both, in order.
	println!(
		"Content-Encoding: {}",
		layer_content_encoding(Some("br"), Coding::Gzip)
	);
}
