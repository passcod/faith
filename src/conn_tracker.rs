use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use napi_derive::napi;
use tokio::sync::RwLock;

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

#[derive(Debug, Clone, Copy, Default)]
pub struct TcpStats {
	pub rtt_us: u32,
	pub rtt_var_us: u32,
	pub lost: u32,
	pub retrans: u32,
	pub total_retrans: u32,
	pub cwnd: u32,
	pub delivery_rate: u64,
}

#[napi(object)]
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
	pub connection_type: String,
	pub local_address: String,
	pub local_port: u16,
	pub remote_address: String,
	pub remote_port: u16,
	pub first_seen: i64,
	pub last_seen: i64,
	pub response_count: i64,
	pub rtt_us: Option<i64>,
	pub rtt_var_us: Option<i64>,
	pub lost_packets: Option<i64>,
	pub retransmits: Option<i64>,
	pub total_retransmits: Option<i64>,
	pub congestion_window: Option<i64>,
	pub delivery_rate_bps: Option<i64>,
}

#[derive(Debug, Default)]
pub struct ConnectionTracker {
	connections: RwLock<HashMap<ConnectionKey, TrackedConnection>>,
	timeout: Duration,
}

impl ConnectionTracker {
	pub fn new(timeout: Duration) -> Arc<Self> {
		Arc::new(Self {
			connections: RwLock::new(HashMap::new()),
			timeout,
		})
	}

	pub async fn track(&self, local_addr: SocketAddr, remote_addr: SocketAddr) {
		let key = ConnectionKey {
			local_addr,
			remote_addr,
		};
		let now = SystemTime::now();

		let mut conns = self.connections.write().await;
		conns
			.entry(key)
			.and_modify(|c| {
				c.last_seen = now;
				c.response_count += 1;
			})
			.or_insert_with(|| TrackedConnection {
				first_seen: now,
				last_seen: now,
				response_count: 1,
				latest_stats: None,
			});
	}

	pub async fn get_all(&self) -> Vec<ConnectionInfo> {
		let conns = self.connections.read().await;

		conns
			.iter()
			.map(|(key, conn)| ConnectionInfo {
				connection_type: "tcp".to_string(),
				local_address: key.local_addr.ip().to_string(),
				local_port: key.local_addr.port(),
				remote_address: key.remote_addr.ip().to_string(),
				remote_port: key.remote_addr.port(),
				first_seen: conn
					.first_seen
					.duration_since(UNIX_EPOCH)
					.unwrap_or_else(|err| err.duration())
					.as_millis()
					.try_into()
					.unwrap_or(i64::MAX),
				last_seen: conn
					.last_seen
					.duration_since(UNIX_EPOCH)
					.unwrap_or_else(|err| err.duration())
					.as_millis()
					.try_into()
					.unwrap_or(i64::MAX),
				response_count: conn.response_count as i64,
				rtt_us: conn.latest_stats.map(|s| s.rtt_us as i64),
				rtt_var_us: conn.latest_stats.map(|s| s.rtt_var_us as i64),
				lost_packets: conn.latest_stats.map(|s| s.lost as i64),
				retransmits: conn.latest_stats.map(|s| s.retrans as i64),
				total_retransmits: conn.latest_stats.map(|s| s.total_retrans as i64),
				congestion_window: conn.latest_stats.map(|s| s.cwnd as i64),
				delivery_rate_bps: conn.latest_stats.map(|s| s.delivery_rate as i64),
			})
			.collect()
	}

	pub async fn update_stats(&self, key: &ConnectionKey, stats: TcpStats) {
		let mut conns = self.connections.write().await;
		if let Some(conn) = conns.get_mut(key) {
			conn.latest_stats = Some(stats);
		}
	}

	pub async fn remove_stale(&self) {
		let mut conns = self.connections.write().await;
		let now = SystemTime::now();
		conns.retain(|_, conn| {
			now.duration_since(conn.last_seen)
				.is_ok_and(|age| age < self.timeout)
		});
	}

	#[allow(dead_code)]
	pub async fn remove(&self, key: &ConnectionKey) {
		let mut conns = self.connections.write().await;
		conns.remove(key);
	}

	pub async fn keys(&self) -> Vec<ConnectionKey> {
		let conns = self.connections.read().await;
		conns.keys().copied().collect()
	}
}
