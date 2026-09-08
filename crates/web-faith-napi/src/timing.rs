//! Surfacing a request's timing as a `PerformanceResourceTiming` for the wrapper.
//!
//! The measuring itself is [`web_faith::timing`]'s; what belongs here is the shape JavaScript
//! receives.
//!
//! spec:RESP#request-timing

use napi_derive::napi;
use web_faith::timing::RequestTiming;

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
