use std::{
	fmt::Debug,
	mem::replace,
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use http_body_util::BodyExt;
use reqwest::Version;
use stream_shared::SharedStream;
use tokio::sync::Mutex;

pub(crate) type DynStream = dyn Stream<Item = std::result::Result<Bytes, String>> + Send + Sync;

pub(crate) enum Body {
	Inner(reqwest::Body),
	Consumed,
	Stream(SharedStream<Pin<Box<DynStream>>>),
}

/// Wrapper around the body that auto-drains on drop to release the connection.
pub(crate) struct BodyHolder {
	pub body: Option<Arc<Mutex<Body>>>,
	/// Flag to prevent drain if body was properly consumed
	pub(crate) drained: Arc<AtomicBool>,
	/// HTTP version - HTTP/2+ doesn't need draining for connection reuse
	pub(crate) version: Version,
}

impl BodyHolder {
	pub fn new(body: Option<Arc<Mutex<Body>>>, version: Version) -> Self {
		Self {
			body,
			version,
			drained: Arc::new(AtomicBool::new(false)),
		}
	}

	pub fn none() -> Self {
		Self {
			body: None,
			version: Version::HTTP_11,
			drained: Arc::new(AtomicBool::new(true)),
		}
	}

	/// Returns true if this is HTTP/2 or HTTP/3 (multiplexed protocols)
	/// where dropping a body doesn't block connection reuse.
	pub fn is_multiplexed(&self) -> bool {
		matches!(self.version, Version::HTTP_2 | Version::HTTP_3)
	}

	/// Mark the body as drained (called when body is fully consumed)
	pub fn mark_drained(&self) {
		self.drained.store(true, Ordering::SeqCst);
	}
}

impl Clone for BodyHolder {
	fn clone(&self) -> Self {
		Self {
			body: self.body.clone(),
			drained: self.drained.clone(),
			version: self.version,
		}
	}
}

impl Debug for BodyHolder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BodyHolder")
			.field("body", &self.body)
			.field("drained", &self.drained.load(Ordering::SeqCst))
			.field("version", &self.version)
			.finish()
	}
}

impl Drop for BodyHolder {
	fn drop(&mut self) {
		if self.drained.load(Ordering::SeqCst) {
			return;
		}

		// For HTTP/2 and HTTP/3, connections are multiplexed - dropping a body
		// stream doesn't prevent connection reuse, so no need to drain.
		if self.is_multiplexed() {
			return;
		}

		if let Some(ref arc) = self.body {
			// Only spawn drain task if we're the last holder
			if Arc::strong_count(arc) == 1 {
				let arc = self.body.take().unwrap();
				// Only spawn if we're in a tokio runtime context
				// (Drop might be called during GC outside of async context)
				if let Ok(handle) = tokio::runtime::Handle::try_current() {
					handle.spawn(async move {
						drain_body_inner(arc).await;
					});
				}
				// If no runtime, the connection will be closed rather than reused
				// This is acceptable as a fallback
			}
		}
	}
}

/// Drain a body to release the connection back to the pool.
/// This reads and discards all remaining bytes.
pub(crate) async fn drain_body_inner(arc: Arc<Mutex<Body>>) {
	let mut guard = arc.lock().await;
	match replace(&mut *guard, Body::Consumed) {
		Body::Inner(body) => {
			let mut body = body;
			while body.frame().await.is_some() {}
		}
		Body::Stream(shared) => {
			futures::pin_mut!(shared);
			while shared.next().await.is_some() {}
		}
		Body::Consumed => {}
	}
}

impl Debug for Body {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Inner(body) => write!(f, "{body:?}"),
			Self::Consumed => write!(f, "Consumed"),
			Self::Stream(stream) => {
				let field = f
					.debug_struct("SharedStream")
					.field("stats", &stream.stats())
					.finish_non_exhaustive();
				f.debug_tuple("Stream").field(&field).finish()
			}
		}
	}
}
