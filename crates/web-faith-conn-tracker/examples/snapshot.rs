//! Track a connection and read back the kernel's own view of it.
//!
//! The addresses here are made up, so the snapshot carries the tracker's own accounting without
//! the per-socket TCP counters a live connection would have. Point it at a real socket pair to see
//! those filled in.
//!
//! Run with `cargo run -p web-faith-conn-tracker --example snapshot`.

use std::{net::SocketAddr, time::Duration};

use web_faith_conn_tracker::ConnectionTracker;

// The tracker spawns a task to refresh the kernel's counters, so it is built inside a runtime.
#[tokio::main]
async fn main() {
	let tracker = ConnectionTracker::new(Duration::from_secs(90));

	let local: SocketAddr = "127.0.0.1:54321".parse().expect("a valid address");
	let remote: SocketAddr = "93.184.216.34:443".parse().expect("a valid address");

	// The first sighting is a new connection; a later one is the pool handing it back, which is
	// what the return value reports.
	println!(
		"first request reused a connection: {}",
		tracker.track(local, remote)
	);
	println!("second request reused it: {}", tracker.track(local, remote));

	// A warm-up's connection is tracked too, so a later request on it counts as a reuse.
	let warmed: SocketAddr = "127.0.0.1:54322".parse().expect("a valid address");
	tracker.track_warmup(warmed, remote);

	for connection in tracker.snapshot() {
		println!(
			"{} {} -> {}: {} responses, os stats: {}",
			connection.connection_type,
			connection.local_addr,
			connection.remote_addr,
			connection.response_count,
			connection.stats.is_some()
		);
	}
}
