//! Per-request timing.

// spec:RESP#request-timing

use std::time::Instant;

use reqwest::{Url, Version};
use tokio::sync::watch;

/// The timing of one request, filled in as it progresses.
///
/// Timings are best-effort: internal limitations mean they are not always perfectly accurate.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct RequestTiming {
	/// Milliseconds from the start of the request to the response head being read.
	pub headers_ms: f64,
	/// Milliseconds from the start of the request to the body finishing, once it has.
	pub body_ms: Option<f64>,
	/// Whether the request travelled on a connection that was already in the pool.
	pub reused: bool,
	/// The ALPN Protocol ID of the protocol the request travelled over.
	pub next_hop_protocol: String,
	/// The response's `Content-Encoding`, captured before a decoded body's header is stripped.
	pub content_encoding: Option<String>,
	/// Whether the response was served by the HTTP cache.
	pub from_cache: bool,
}

/// Where the timing lands: written by whoever finishes the body, awaited by `timing()`.
///
/// A watch channel for the same reason the trailers slot is one: a body that is never read never
/// finishes, so the wait is unbounded.
#[derive(Debug)]
pub struct TimingSlot {
	tx: watch::Sender<RequestTiming>,
	started: Instant,
}

impl TimingSlot {
	pub fn new(started: Instant, timing: RequestTiming) -> Self {
		Self {
			tx: watch::channel(timing).0,
			started,
		}
	}

	/// Record that the body ended, if nothing got there first.
	///
	/// `send_if_modified` keeps the read and write one step, and wakes waiters only from the call
	/// that settled it. Every route out of a body lands here: the stream ending, `discard()`, and
	/// the collector draining an abandoned body.
	pub fn ended(&self) {
		let elapsed = self.started.elapsed().as_secs_f64() * 1000.0;
		self.tx.send_if_modified(|timing| {
			if timing.body_ms.is_none() {
				timing.body_ms = Some(elapsed);
				true
			} else {
				false
			}
		});
	}

	/// Wait until the body has finished.
	pub async fn settled(&self) -> RequestTiming {
		let mut rx = self.tx.subscribe();
		// `wait_for` tests the current value before waiting, so a body that already finished
		// returns without yielding. Its error case is the sender being gone, which means the
		// response was dropped with the body unread: the phases reached before that are all
		// there is to report, so report them rather than waiting for a moment that can no
		// longer come.
		let settled = match rx.wait_for(|timing| timing.body_ms.is_some()).await {
			Ok(timing) => Some(timing.clone()),
			Err(_) => None,
		};
		settled.unwrap_or_else(|| rx.borrow().clone())
	}
}

/// The ALPN Protocol ID (RFC 7301) for the protocol a response travelled over.
///
/// Reported whether or not ALPN negotiated it, as a browser does: cleartext HTTP/2 is `h2c` and
/// cleartext HTTP/1.1 is `http/1.1`.
pub(crate) fn alpn_protocol_id(version: Version, url: &Url) -> String {
	let secure = url.scheme() == "https";
	match version {
		Version::HTTP_3 => "h3",
		Version::HTTP_2 => {
			if secure {
				"h2"
			} else {
				"h2c"
			}
		}
		Version::HTTP_11 => "http/1.1",
		Version::HTTP_10 => "http/1.0",
		Version::HTTP_09 => "http/0.9",
		_ => "",
	}
	.to_owned()
}
