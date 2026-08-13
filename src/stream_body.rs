use bytes::Bytes;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::sync::Arc;
use tokio::sync::mpsc;

/// A sender that allows JavaScript to push chunks into a Rust stream.
/// This bypasses NAPI-rs's buggy ReadableStream Reader by letting JS
/// drive the chunk delivery explicitly.
#[napi]
pub struct StreamBodySender {
	tx: Option<mpsc::Sender<Bytes>>,
}

#[napi]
impl StreamBodySender {
	/// Push a chunk of data into the stream.
	/// Returns true if the chunk was sent successfully, false if the receiver was dropped.
	#[napi]
	pub async fn push(&self, chunk: Buffer) -> napi::Result<bool> {
		let Some(tx) = &self.tx else {
			return Ok(false);
		};

		let bytes = Bytes::copy_from_slice(chunk.as_ref());
		match tx.send(bytes).await {
			Ok(()) => Ok(true),
			Err(_) => Ok(false), // Receiver dropped
		}
	}

	/// Close the stream, signaling that no more chunks will be sent.
	#[napi]
	pub fn close(&mut self) -> napi::Result<()> {
		// Drop the sender to close the channel
		self.tx.take();
		Ok(())
	}
}

/// Internal receiver that can be converted into a stream for reqwest
pub struct StreamBodyReceiver {
	rx: mpsc::Receiver<Bytes>,
}

impl StreamBodyReceiver {
	/// Convert this receiver into a Stream suitable for reqwest::Body
	pub fn into_stream(
		self,
	) -> impl futures::Stream<Item = std::result::Result<Bytes, std::io::Error>> + Send {
		async_stream::stream! {
			let mut rx = self.rx;
			while let Some(bytes) = rx.recv().await {
				yield std::result::Result::<Bytes, std::io::Error>::Ok(bytes);
			}
		}
	}
}

// We need to use Arc to share the receiver with the fetch function
pub type SharedStreamBodyReceiver = Arc<tokio::sync::Mutex<Option<StreamBodyReceiver>>>;

/// A streaming body that can be passed to fetch().
/// Create one with createStreamBodyPair(), then use the returned sender to push chunks.
#[napi]
pub struct StreamBody {
	pub(crate) receiver: SharedStreamBodyReceiver,
}

/// Create a paired StreamBody and StreamBodySender for streaming request bodies.
///
/// Usage:
/// ```js
/// const { body, sender } = createStreamBodyPair();
/// // Start the fetch with the body
/// const responsePromise = fetch(url, { method: 'POST', body, duplex: 'half' });
/// // Push chunks asynchronously
/// await sender.push(Buffer.from('chunk1'));
/// await sender.push(Buffer.from('chunk2'));
/// // Close when done
/// sender.close();
/// // Wait for response
/// const response = await responsePromise;
/// ```
#[napi(ts_return_type = "{ body: StreamBody, sender: StreamBodySender }")]
pub fn create_stream_body_pair<'env>(
	env: &'env Env,
	buffer_size: Option<u32>,
) -> napi::Result<Object<'env>> {
	let size = buffer_size.unwrap_or(16) as usize;
	let (tx, rx) = mpsc::channel(size);

	let receiver = StreamBodyReceiver { rx };
	let body = StreamBody {
		receiver: Arc::new(tokio::sync::Mutex::new(Some(receiver))),
	};
	let sender = StreamBodySender { tx: Some(tx) };

	let mut obj = Object::new(env)?;
	obj.set("body", body)?;
	obj.set("sender", sender)?;

	Ok(obj)
}
