use napi::{
	Env,
	bindgen_prelude::{AbortSignal, PromiseRaw},
};
use napi_derive::napi;
use tokio::sync::mpsc;

use bytes::Bytes;
use web_faith::request::{self, RequestBody};

use crate::{
	async_task::faith_promise,
	options::{self, FaithOptionsAndBody},
	response::FaithResponse,
	stream_body::StreamBody,
};

#[napi]
pub fn faith_fetch<'env>(
	env: &'env Env,
	url: String,
	options: FaithOptionsAndBody,
	signal: Option<AbortSignal>,
	stream_body: Option<&StreamBody>,
) -> Result<PromiseRaw<'env, FaithResponse>, napi::Error> {
	let (options, agent, body) = options::extract(options);
	let (s, abort) = mpsc::channel(8);
	let has_signal = signal.is_some();
	if let Some(signal) = signal {
		signal.on_abort(move || {
			let _ = s.try_send(());
		});
	}

	// Get the stream body receiver if provided
	let stream_receiver = stream_body.map(|sb| sb.receiver.clone());

	faith_promise(env, async move {
		let body = match (stream_receiver, body) {
			// A streaming body is taken from the sender's channel; the receiver is held behind a
			// lock because JavaScript hands the same stream body object to one request only.
			(Some(receiver), _) => match receiver.lock().await.take() {
				Some(receiver) => RequestBody::Stream(Box::pin(receiver.into_stream())),
				None => RequestBody::None,
			},
			(None, Some(buffer)) => RequestBody::Bytes(Bytes::copy_from_slice(&buffer)),
			(None, None) => RequestBody::None,
		};

		let abort = has_signal.then(|| async move {
			let mut abort = abort;
			let _ = abort.recv().await;
		});

		request::send(&agent.inner, &url, options, body, abort)
			.await
			.map(FaithResponse::from)
	})
}
