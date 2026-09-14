//! Warm the cache for a name, then read back which servers the lookup went through.
//!
//! Resolves against the operating system's own servers, since those are what a machine running
//! this is configured for. Pass a name to look up something other than `localhost`.
//!
//! Run with `cargo run -p web-faith-dns --example resolve -- example.com`.

use web_faith_dns::{FaithResolver, ResolverConfig};

#[tokio::main]
async fn main() {
	let host = std::env::args()
		.nth(1)
		.unwrap_or_else(|| "localhost".into());

	// No servers named, so the resolver configures itself from the operating system and lets
	// opportunistic encryption upgrade those servers where it can.
	let resolver = FaithResolver::new(ResolverConfig::default());

	// Prefetching populates the very cache a request's lookup reads, and never fails: warming is
	// advisory, so a name that does not resolve simply leaves the cache as it was.
	resolver.prefetch(&host).await;

	// The configuration is read on first use, so this is empty until something has resolved.
	let reports = resolver.resolvers();
	if reports.is_empty() {
		println!("nothing resolved through Faith's own servers for {host}");
	}
	for report in reports {
		println!(
			"{} over {} ({})",
			report.address, report.transport, report.source
		);
	}

	// A network change discards what was learned from a network that no longer exists, leaving the
	// resolver usable rather than needing to be rebuilt.
	resolver.reset();
	println!(
		"after a network change: {} servers",
		resolver.resolvers().len()
	);
}
