//! Live per-connection TCP statistics, read from the operating system.
//!
//! TCP connections are done via OS primitives, and reqwest's pool doesn't expose statistics and
//! tracking information directly. But we can read the kernel tables to obtain these. Supports
//! Linux, macOS, Windows. Other platforms return empty stats.
//!
//! Statistics refresh once a second, and a connection idle for longer than the tracker's timeout
//! expires out of it.
//!
//! ```no_run
//! use std::{net::SocketAddr, time::Duration};
//!
//! use web_faith_conn_tracker::ConnectionTracker;
//!
//! // Spawns a refresh task, so build it inside a tokio runtime.
//! let tracker = ConnectionTracker::new(Duration::from_secs(90));
//!
//! let local: SocketAddr = "127.0.0.1:54321".parse().expect("a valid address");
//! let remote: SocketAddr = "93.184.216.34:443".parse().expect("a valid address");
//!
//! // Returns whether this connection had been seen before, so a repeat means it was reused.
//! println!("reused an existing connection: {}", tracker.track(local, remote));
//!
//! for connection in tracker.snapshot() {
//!     let Some(stats) = connection.stats else { continue };
//!     println!(
//!         "{}: rtt {}us, cwnd {}, {} retransmits",
//!         connection.remote_addr, stats.rtt_us, stats.cwnd, stats.total_retrans,
//!     );
//! }
//! ```

#![deny(missing_docs)]
// Lets docs.rs label each item with the feature or platform it needs.
#![cfg_attr(docsrs, feature(doc_cfg))]

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
/// The address pair identifying one connection.
pub struct ConnectionKey {
	/// This end of the connection.
	pub local_addr: SocketAddr,
	/// The peer.
	pub remote_addr: SocketAddr,
}

#[derive(Debug, Clone)]
pub(crate) struct TrackedConnection {
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
#[non_exhaustive]
/// The kernel's view of one TCP connection.
///
/// Which fields are populated depends on the platform, and none is guaranteed to keep being
/// populated across releases.
pub struct TcpStats {
	/// Smoothed round-trip time, in microseconds.
	pub rtt_us: u32,
	/// Round-trip time variance, in microseconds.
	pub rtt_var_us: u32,
	/// Packets the kernel considers lost. Linux only.
	pub lost: Option<u32>,
	/// Segments retransmitted on the current send.
	pub retrans: u32,
	/// Segments retransmitted over the connection's life.
	pub total_retrans: u32,
	/// Congestion window, in segments.
	pub cwnd: u32,
	/// Most recent delivery rate, in bytes per second. Linux only.
	pub delivery_rate: Option<u64>,
}

/// One tracked connection.
///
/// A snapshot, as the tracker saw things when it was taken, rather than a live view of the system:
/// the timestamps and counts are the tracker's own accounting, and `stats` is whatever the kernel
/// last reported, refreshed once a second.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConnectionSnapshot {
	/// The transport this connection runs over. At the moment this is always `"tcp"`.
	pub connection_type: &'static str,
	/// This end of the connection.
	pub local_addr: SocketAddr,
	/// The peer.
	pub remote_addr: SocketAddr,
	/// When the tracker first saw traffic on this connection.
	pub first_seen: SystemTime,
	/// When the tracker last saw traffic on this connection.
	pub last_seen: SystemTime,
	/// When this connection falls out of the tracker, unless traffic renews it first.
	pub expiry: Option<SystemTime>,
	/// Responses that have arrived over this connection.
	pub response_count: u64,
	/// The operating system's last report for this connection.
	///
	/// This can be `None` on a platform with no support, in the first second of a connection's
	/// life before the refresh has run, or when the kernel's table no longer carries it.
	pub stats: Option<TcpStats>,
}

type Conns = Cache<ConnectionKey, TrackedConnection>;

/// Tracks a set of TCP connections, and reads the kernel's statistics for them.
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
	/// A tracker that drops a connection once it has been idle for `timeout`.
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

	/// Record traffic on a connection, returning whether it was already known.
	///
	/// A connection the tracker has seen before was reused rather than newly dialled, a fresh one
	/// taking a local port of its own.
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

	/// Register a connection opened ahead of the traffic that will use it.
	///
	/// It is listed at a count of zero until traffic arrives on it. A connection already tracked is
	/// left untouched, so this cannot disturb the count or timestamps of one already in use.
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
