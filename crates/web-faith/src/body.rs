//! Response bodies.

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

use crate::timing::TimingSlot;

/// A body byte-stream, as a response hands one out.
pub type DynStream = dyn Stream<Item = std::result::Result<Bytes, String>> + Send + Sync;

/// A response body, in whichever state it has reached.
pub enum Body {
	/// As it arrived, not yet read.
	Inner(reqwest::Body),
	/// Read to the end, or discarded.
	Consumed,
	/// Handed out as a stream that a response and its clones share.
	Stream(SharedStream<Pin<Box<DynStream>>>),
}

/// A response body, drained on drop so its connection goes back to the pool.
///
/// An HTTP/1 connection can't be reused until its body has been read to the end.
pub struct BodyHolder {
	/// `None` for a response that cannot carry a body.
	pub body: Option<Arc<Mutex<Body>>>,
	/// Set once the body has been consumed, so dropping it drains nothing.
	pub drained: Arc<AtomicBool>,
	/// Which protocol carried the response, since only HTTP/1 needs the drain.
	pub version: Version,
	/// Settled when the body ends, so an abandoned body still finishes its timing.
	pub timing: Option<Arc<TimingSlot>>,
}

impl BodyHolder {
	/// Hold `body`, draining it on drop if `version` needs that to reuse the connection.
	pub fn new(body: Option<Arc<Mutex<Body>>>, version: Version, timing: Arc<TimingSlot>) -> Self {
		Self {
			body,
			version,
			drained: Arc::new(AtomicBool::new(false)),
			timing: Some(timing),
		}
	}

	/// A holder for a response that cannot carry a body.
	pub fn none() -> Self {
		Self {
			body: None,
			version: Version::HTTP_11,
			drained: Arc::new(AtomicBool::new(true)),
			timing: None,
		}
	}

	/// Whether the response came over HTTP/2 or HTTP/3, where dropping a body cancels its stream
	/// and leaves the connection alone, so there is nothing to drain.
	pub fn is_multiplexed(&self) -> bool {
		matches!(self.version, Version::HTTP_2 | Version::HTTP_3)
	}

	/// Note that the body has been consumed, so dropping it drains nothing.
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
			timing: self.timing.clone(),
		}
	}
}

impl Debug for BodyHolder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BodyHolder")
			.field("body", &self.body)
			.field("drained", &self.drained.load(Ordering::SeqCst))
			.field("version", &self.version)
			.field("timing", &self.timing)
			.finish()
	}
}

impl Drop for BodyHolder {
	fn drop(&mut self) {
		if self.drained.load(Ordering::SeqCst) {
			return;
		}

		// Only the last holder ends the body: a clone going away while another still holds
		// it settles nothing, since that one may yet read it.
		if self
			.body
			.as_ref()
			.is_some_and(|arc| Arc::strong_count(arc) > 1)
		{
			return;
		}

		// An abandoned body still ends here, so its timing settles rather than waiting for a
		// read that is never coming (spec:RESP#request-timing).
		let timing = self.timing.take();

		// For HTTP/2 and HTTP/3, connections are multiplexed - dropping a body
		// stream doesn't prevent connection reuse, so no need to drain.
		if self.is_multiplexed() {
			if let Some(timing) = timing {
				timing.ended();
			}
			return;
		}

		if let Some(arc) = self.body.take() {
			// Only spawn if we're in a tokio runtime context
			// (Drop might be called during GC outside of async context)
			if let Ok(handle) = tokio::runtime::Handle::try_current() {
				handle.spawn(async move {
					drain_body_inner(arc).await;
					// The drain is what ends this body, so the timing settles on its
					// last byte rather than on the collector noticing.
					if let Some(timing) = timing {
						timing.ended();
					}
				});
			} else if let Some(timing) = timing {
				// If no runtime, the connection will be closed rather than reused
				// This is acceptable as a fallback
				timing.ended();
			}
		} else if let Some(timing) = timing {
			timing.ended();
		}
	}
}

/// Read and discard whatever is left of a body, so its connection goes back to the pool.
pub async fn drain_body_inner(arc: Arc<Mutex<Body>>) {
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
