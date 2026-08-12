//! Per-request timing, surfaced as a `PerformanceResourceTiming` by the wrapper.
//!
//! spec:RESP#request-timing

use std::{
	sync::{Arc, OnceLock},
	time::Instant,
};

use napi_derive::napi;
use reqwest::{Url, Version};
use tokio::sync::watch;

/// The moment a response's headers arrived, shared between the middleware that observes it and
/// the request that surfaces it.
///
/// Carried in the request's extensions so the one stamp taken inside the stack reaches the
/// outside, which is what keeps the surfaced timing and the path-time average reading the same
/// measurement rather than two of their own.
#[derive(Clone, Debug, Default)]
pub struct HeadersStamp(Arc<OnceLock<Instant>>);

impl HeadersStamp {
	/// Record the arrival, if this is the first response to reach the outside.
	///
	/// An HTTP/3 attempt that fails and falls back to TCP runs the stack twice, and only the
	/// attempt that produced the response stamps, so the recorded moment always belongs to the
	/// response the caller receives.
	///
	/// The stamping lives in the Alt-Svc layer, which is only built with HTTP/3 support; without
	/// it nothing stamps and the request falls back to timing the send itself.
	#[cfg_attr(not(feature = "http3"), allow(dead_code))]
	pub fn mark(&self, at: Instant) {
		let _ = self.0.set(at);
	}

	pub fn get(&self) -> Option<Instant> {
		self.0.get().copied()
	}
}

/// What Fáith measures of a request, filled in as the request progresses.
#[derive(Clone, Debug, Default)]
pub struct RequestTiming {
	/// Milliseconds from the start of the request to the response headers arriving.
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
/// A watch channel for the same reason the trailers slot is one: the wait is unbounded by
/// design, since a body that is never read never finishes, and polling would burn a core to
/// find that out.
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
	/// `send_if_modified` so the read and the write are one step, and so waiters are woken only
	/// by the call that actually settled it. Every route out of a body lands here: the stream
	/// ending, `discard()`, and the collector draining one that was abandoned.
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

/// The measurements behind a response's timing breakdown.
///
/// The wrapper turns these into a `PerformanceResourceTiming`; the phases are milliseconds from
/// the start of the request rather than absolute times, so the wrapper can place them on the
/// same clock as the rest of the platform's performance entries.
#[napi(object)]
#[derive(Clone, Debug)]
pub struct TimingBreakdown {
	pub headers_ms: f64,
	pub body_ms: Option<f64>,
	pub reused: bool,
	pub next_hop_protocol: String,
	pub content_encoding: Option<String>,
	pub from_cache: bool,
}

impl From<RequestTiming> for TimingBreakdown {
	fn from(timing: RequestTiming) -> Self {
		Self {
			headers_ms: timing.headers_ms,
			body_ms: timing.body_ms,
			reused: timing.reused,
			next_hop_protocol: timing.next_hop_protocol,
			content_encoding: timing.content_encoding,
			from_cache: timing.from_cache,
		}
	}
}

/// The ALPN Protocol ID (RFC 7301) naming the protocol a response travelled over.
///
/// Reported whether or not the connection negotiated over ALPN, which is what a browser does:
/// cleartext HTTP/2 is `h2c` and cleartext HTTP/1.1 is still `http/1.1`, neither of which any
/// handshake agreed on.
pub fn alpn_protocol_id(version: Version, url: &Url) -> String {
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
