#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use moka::Expiry;
use moka::{ops::compute::Op, sync::Cache};
use napi::{Env, JsDate};
use napi_derive::napi;
use tokio::{spawn, task::AbortHandle, time::sleep};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionKey {
	pub local_addr: SocketAddr,
	pub remote_addr: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct TrackedConnection {
	pub first_seen: SystemTime,
	pub last_seen: SystemTime,
	pub response_count: u64,
	pub latest_stats: Option<TcpStats>,
}

struct ExpireAfterTimeout(Duration);
impl Expiry<ConnectionKey, TrackedConnection> for ExpireAfterTimeout {
	fn expire_after_create(
		&self,
		_key: &ConnectionKey,
		_value: &TrackedConnection,
		_created_at: Instant,
	) -> Option<Duration> {
		Some(self.0)
	}

	fn expire_after_read(
		&self,
		_key: &ConnectionKey,
		value: &TrackedConnection,
		_read_at: Instant,
		_duration_until_expiry: Option<Duration>,
		_last_modified_at: Instant,
	) -> Option<Duration> {
		Some(
			self.0
				.saturating_sub(value.last_seen.elapsed().unwrap_or_default()),
		)
	}

	fn expire_after_update(
		&self,
		_key: &ConnectionKey,
		value: &TrackedConnection,
		_updated_at: Instant,
		_duration_until_expiry: Option<Duration>,
	) -> Option<Duration> {
		Some(
			self.0
				.saturating_sub(value.last_seen.elapsed().unwrap_or_default()),
		)
	}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TcpStats {
	pub rtt_us: u32,
	pub rtt_var_us: u32,
	pub lost: Option<u32>,
	pub retrans: u32,
	pub total_retrans: u32,
	pub cwnd: u32,
	pub delivery_rate: Option<u64>,
}

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

type Conns = Cache<ConnectionKey, TrackedConnection>;

#[derive(Debug)]
pub struct ConnectionTracker {
	connections: Conns,
	timeout: Duration,
	task_abort: AbortHandle,
}

impl Drop for ConnectionTracker {
	fn drop(&mut self) {
		self.task_abort.abort();
	}
}

impl ConnectionTracker {
	pub fn new(timeout: Duration) -> Arc<Self> {
		let connections = Cache::builder()
			.expire_after(ExpireAfterTimeout(timeout))
			.build();

		let conns = connections.clone();
		let task_abort = spawn(async move {
			loop {
				let _ = update_all(conns.clone());
				sleep(Duration::from_secs(1)).await;
			}
		})
		.abort_handle();

		Arc::new(Self {
			connections,
			timeout,
			task_abort,
		})
	}

	pub fn track(&self, local_addr: SocketAddr, remote_addr: SocketAddr) {
		let now = SystemTime::now();
		self.connections.insert(
			ConnectionKey {
				local_addr,
				remote_addr,
			},
			TrackedConnection {
				first_seen: now,
				last_seen: now,
				response_count: 1,
				latest_stats: None,
			},
		);
	}

	pub fn get_for_napi<'env>(&self, env: &'env Env) -> Vec<ConnectionInfo<'env>> {
		self.connections
			.iter()
			.map(|(key, conn)| ConnectionInfo {
				connection_type: "tcp".to_string(),
				local_address: key.local_addr.ip().to_string(),
				local_port: key.local_addr.port(),
				remote_address: key.remote_addr.ip().to_string(),
				remote_port: key.remote_addr.port(),
				first_seen: env
					.create_date(
						conn.first_seen
							.duration_since(UNIX_EPOCH)
							.unwrap_or_else(|err| err.duration())
							.as_secs_f64() * 1000.0,
					)
					.ok(),
				last_seen: env
					.create_date(
						conn.last_seen
							.duration_since(UNIX_EPOCH)
							.unwrap_or_else(|err| err.duration())
							.as_secs_f64() * 1000.0,
					)
					.ok(),
				expiry: conn.last_seen.checked_add(self.timeout).and_then(|exp| {
					env.create_date(
						exp.duration_since(UNIX_EPOCH)
							.unwrap_or_else(|err| err.duration())
							.as_secs_f64() * 1000.0,
					)
					.ok()
				}),
				response_count: conn.response_count as i64,
				rtt_us: conn.latest_stats.map(|s| s.rtt_us as i64),
				rtt_var_us: conn.latest_stats.map(|s| s.rtt_var_us as i64),
				lost_packets: conn.latest_stats.and_then(|s| s.lost.map(|v| v as i64)),
				retransmits: conn.latest_stats.map(|s| s.retrans as i64),
				total_retransmits: conn.latest_stats.map(|s| s.total_retrans as i64),
				congestion_window: conn.latest_stats.map(|s| s.cwnd as i64),
				delivery_rate_bps: conn
					.latest_stats
					.and_then(|s| s.delivery_rate.map(|v| v as i64)),
			})
			.collect()
	}
}

fn update_all(conns: Conns) -> std::io::Result<()> {
	let keys: Vec<ConnectionKey> = conns.iter().map(|(k, _)| *k).collect();
	if keys.is_empty() {
		return Ok(());
	}

	#[allow(
		unused_variables,
		reason = "when any of the platform-specific impls work, this will be shadowed"
	)]
	let stats: Vec<(ConnectionKey, TcpStats)> = Vec::new();

	#[cfg(target_os = "linux")]
	let stats = linux::query_tcp_stats(&keys)?;

	#[cfg(target_os = "macos")]
	let stats = macos::query_tcp_stats(&keys)?;

	#[cfg(target_os = "windows")]
	let stats = windows::query_tcp_stats(&keys)?;

	for (key, tcp_stats) in &stats {
		update_stats(&conns, *key, *tcp_stats);
	}

	Ok(())
}

fn update_stats(conns: &Conns, key: ConnectionKey, stats: TcpStats) {
	conns.entry(key).and_compute_with(|entry| {
		if let Some(entry) = entry {
			let mut entry = entry.into_value();
			entry.latest_stats = Some(stats);
			Op::Put(entry)
		} else {
			Op::Nop
		}
	});
}
