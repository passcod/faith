//! Live per-connection TCP statistics, read from the operating system.
//!
//! A connection pool can say which connections it holds, but not how any of them is actually
//! behaving: round-trip time, retransmits, congestion window, delivery rate. The kernel knows, and
//! this reads it.
//!
//! Report traffic on a connection with [`ConnectionTracker::track`], which also answers whether that
//! connection had been seen before, and take the current view with
//! [`ConnectionTracker::snapshot`]. Each entry's statistics are refreshed once a second, and an
//! entry that goes idle for longer than the configured timeout expires out of the tracker.
//!
//! Reading the statistics is per-platform: Linux over netlink, macOS and Windows through their own
//! interfaces. Anywhere else, connections are still tracked but carry no statistics.

// spec:OBS

#[cfg(target_os = "linux")]
#[path = "platform/linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "platform/macos.rs"]
mod macos;
#[cfg(target_os = "windows")]
#[path = "platform/windows.rs"]
mod windows;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use moka::Expiry;
use moka::{ops::compute::Op, sync::Cache};
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

/// One tracked connection, as a caller reporting on the pool sees it.
#[derive(Debug, Clone)]
pub struct ConnectionSnapshot {
	/// The transport the connection runs over. Only TCP is tracked.
	pub connection_type: &'static str,
	pub local_addr: SocketAddr,
	pub remote_addr: SocketAddr,
	pub first_seen: SystemTime,
	pub last_seen: SystemTime,
	/// When the entry falls out of the tracker, unless traffic renews it first.
	pub expiry: Option<SystemTime>,
	pub response_count: u64,
	/// What the operating system last reported for this connection, if it has been asked yet.
	pub stats: Option<TcpStats>,
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

	/// Record a response on a connection, returning whether that connection was already known.
	///
	/// A connection the tracker has seen before is one the pool handed back rather than one
	/// dialled for this request, since a fresh connection takes a local port of its own.
	pub fn track(&self, local_addr: SocketAddr, remote_addr: SocketAddr) -> bool {
		let now = SystemTime::now();
		let key = ConnectionKey {
			local_addr,
			remote_addr,
		};
		let mut known = false;
		self.connections.entry(key).and_compute_with(|entry| {
			if let Some(entry) = entry {
				known = true;
				let mut conn = entry.into_value();
				conn.last_seen = now;
				conn.response_count += 1;
				Op::Put(conn)
			} else {
				Op::Put(TrackedConnection {
					first_seen: now,
					last_seen: now,
					response_count: 1,
					latest_stats: None,
				})
			}
		});
		known
	}

	/// Register a warm-up connection that no request has yet been credited to.
	///
	/// A warm-up connection is listed before any foreground request uses it, at a response count of
	/// zero. An entry already tracked is left untouched: a warm-up to an origin that already holds a
	/// pooled connection does no new work, and must not disturb the count or timestamps of the
	/// connection it would reuse.
	// spec:WARM
	pub fn track_warmup(&self, local_addr: SocketAddr, remote_addr: SocketAddr) {
		let now = SystemTime::now();
		let key = ConnectionKey {
			local_addr,
			remote_addr,
		};
		self.connections.entry(key).and_compute_with(|entry| {
			if entry.is_some() {
				Op::Nop
			} else {
				Op::Put(TrackedConnection {
					first_seen: now,
					last_seen: now,
					response_count: 0,
					latest_stats: None,
				})
			}
		});
	}

	/// Every connection currently tracked.
	pub fn snapshot(&self) -> Vec<ConnectionSnapshot> {
		self.connections
			.iter()
			.map(|(key, conn)| ConnectionSnapshot {
				connection_type: "tcp",
				local_addr: key.local_addr,
				remote_addr: key.remote_addr,
				first_seen: conn.first_seen,
				last_seen: conn.last_seen,
				expiry: conn.last_seen.checked_add(self.timeout),
				response_count: conn.response_count,
				stats: conn.latest_stats,
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
