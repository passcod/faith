//! Handing the connection tracker's view to JavaScript. (spec:OBS)
//!
//! The tracking itself, and reading the operating system's statistics, is
//! [`web_faith_conn_tracker`]'s; what belongs here is turning a snapshot into an object V8 can
//! carry, which means JavaScript `Date`s for the timestamps and `i64` for every count.

use std::time::{SystemTime, UNIX_EPOCH};

use napi::{Env, JsDate};
use napi_derive::napi;
use web_faith_conn_tracker::{ConnectionSnapshot, ConnectionTracker};

#[napi(object)]
#[derive(Clone)]
pub struct ConnectionInfo<'env> {
	pub connection_type: String,
	pub local_address: String,
	pub local_port: u16,
	pub remote_address: String,
	pub remote_port: u16,
	pub first_seen: Option<JsDate<'env>>,
	pub last_seen: Option<JsDate<'env>>,
	pub expiry: Option<JsDate<'env>>,
	pub response_count: i64,
	pub rtt_us: Option<i64>,
	pub rtt_var_us: Option<i64>,
	pub lost_packets: Option<i64>,
	pub retransmits: Option<i64>,
	pub total_retransmits: Option<i64>,
	pub congestion_window: Option<i64>,
	pub delivery_rate_bps: Option<i64>,
}

/// A JavaScript `Date` for a point in time, which is milliseconds since the epoch.
///
/// A timestamp from before the epoch yields the difference the error carries, matching what this
/// did when the conversion lived on the tracker.
fn js_date<'env>(env: &'env Env, at: SystemTime) -> Option<JsDate<'env>> {
	env.create_date(
		at.duration_since(UNIX_EPOCH)
			.unwrap_or_else(|err| err.duration())
			.as_secs_f64()
			* 1000.0,
	)
	.ok()
}

/// The tracker's view, as `agent.connections()` returns it.
pub fn connections_for_napi<'env>(
	tracker: &ConnectionTracker,
	env: &'env Env,
) -> Vec<ConnectionInfo<'env>> {
	tracker
		.snapshot()
		.into_iter()
		.map(|conn| {
			let ConnectionSnapshot { stats, .. } = &conn;
			ConnectionInfo {
				connection_type: conn.connection_type.to_string(),
				local_address: conn.local_addr.ip().to_string(),
				local_port: conn.local_addr.port(),
				remote_address: conn.remote_addr.ip().to_string(),
				remote_port: conn.remote_addr.port(),
				first_seen: js_date(env, conn.first_seen),
				last_seen: js_date(env, conn.last_seen),
				expiry: conn.expiry.and_then(|at| js_date(env, at)),
				response_count: conn.response_count as i64,
				rtt_us: stats.map(|s| s.rtt_us as i64),
				rtt_var_us: stats.map(|s| s.rtt_var_us as i64),
				lost_packets: stats.and_then(|s| s.lost.map(|v| v as i64)),
				retransmits: stats.map(|s| s.retrans as i64),
				total_retransmits: stats.map(|s| s.total_retrans as i64),
				congestion_window: stats.map(|s| s.cwnd as i64),
				delivery_rate_bps: stats.and_then(|s| s.delivery_rate.map(|v| v as i64)),
			}
		})
		.collect()
}
