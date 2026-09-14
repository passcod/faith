use std::{
	net::IpAddr,
	sync::Arc,
	time::{Duration, Instant},
};

use super::{FaithResolver, StaleEntry};
use crate::{settings::ResolverConfig, transport::ServerSpec};

fn spec(input: &str) -> ServerSpec {
	input.parse::<ServerSpec>().expect("valid server URL")
}

#[tokio::test]
async fn reset_replaces_the_generation_and_what_it_holds() {
	// A network change drops what was read off the old network, so the next lookup builds
	// against the new one rather than reusing the previous network's servers (spec:NETCHG).
	let resolver = FaithResolver::new(ResolverConfig {
		servers: vec![spec("udp://127.0.0.1:1")],
		timeout: Some(Duration::from_millis(200)),
		..ResolverConfig::default()
	});

	let before = resolver.generation();
	// Build the generation's state, so there is something for the reset to drop.
	let _ = resolver.built(&before).await;
	assert!(
		before.built.get().is_some(),
		"the generation built its resolver"
	);
	assert_eq!(resolver.resolvers().len(), 1, "which `resolvers()` reports");

	resolver.reset();

	let after = resolver.generation();
	assert!(
		!Arc::ptr_eq(&before, &after),
		"the reset swaps the generation rather than mutating it"
	);
	assert!(
		after.built.get().is_none(),
		"the new generation holds nothing until it is used again"
	);
	assert!(
		before.built.get().is_some(),
		"work already holding the old generation keeps its resolvers"
	);
	assert!(
		resolver.resolvers().is_empty(),
		"`resolvers()` reports nothing until the rebuild (spec:OBS#resolvers)"
	);

	// Configuration survives the signal, so the rebuild uses the servers as configured.
	let _ = resolver.built(&after).await;
	assert_eq!(
		resolver.resolvers().len(),
		1,
		"the rebuilt generation resolves through the configured servers again"
	);
}

/// A resolver with a stale entry for `host` whose freshness ended `ago`.
fn with_stale_entry(config: ResolverConfig, host: &str, ago: Duration) -> FaithResolver {
	let resolver = FaithResolver::new(config);
	resolver.generation().stale.insert(
		host.to_owned(),
		StaleEntry {
			addrs: Arc::new(vec![IpAddr::from([127, 0, 0, 1])]),
			valid_until: Instant::now() - ago,
		},
	);
	resolver
}

#[test]
fn only_an_expired_entry_inside_the_window_is_served_stale() {
	// spec:DNS#serving-stale-answers
	let config = || ResolverConfig {
		serve_stale: Some(Duration::from_secs(60)),
		..ResolverConfig::default()
	};

	// Still fresh: the lookup goes through hickory, which answers from its own cache.
	let fresh = FaithResolver::new(config());
	fresh.generation().stale.insert(
		"fresh.test".to_owned(),
		StaleEntry {
			addrs: Arc::new(vec![IpAddr::from([127, 0, 0, 1])]),
			valid_until: Instant::now() + Duration::from_secs(60),
		},
	);
	let generation = fresh.generation();
	assert!(
		fresh.stale_addrs(&generation, "fresh.test").is_none(),
		"a fresh entry is not a stale hit"
	);

	// Expired but inside `dns.maxStale`: served immediately.
	let stale = with_stale_entry(config(), "stale.test", Duration::from_secs(5));
	let generation = stale.generation();
	assert!(
		stale.stale_addrs(&generation, "stale.test").is_some(),
		"an entry expired inside the window is served"
	);

	// Past the window: no longer evidence about the host, so the lookup blocks.
	let old = with_stale_entry(config(), "old.test", Duration::from_secs(120));
	let generation = old.generation();
	assert!(
		old.stale_addrs(&generation, "old.test").is_none(),
		"an entry past `dns.maxStale` is not served"
	);
	assert!(
		generation.stale.get("old.test").is_none(),
		"and is dropped rather than left to age further"
	);
}

#[test]
fn serve_stale_off_never_serves_an_expired_entry() {
	// spec:DNS#serving-stale-answers — the switch for a caller that must not connect to an
	// address it knows to be out of date.
	let resolver = with_stale_entry(
		ResolverConfig {
			serve_stale: None,
			..ResolverConfig::default()
		},
		"strict.test",
		Duration::from_secs(5),
	);
	let generation = resolver.generation();
	assert!(resolver.stale_addrs(&generation, "strict.test").is_none());
	assert!(
		!resolver.served_stale("strict.test"),
		"and nothing is reported as stale-served, so no retry is armed"
	);
}

#[test]
fn served_stale_tracks_the_window_it_serves_from() {
	// The retry layer arms itself from this, so it must not claim an address was assumed when
	// the lookup actually blocked on a fresh one (spec:DNS#when-a-stale-address-is-wrong).
	let config = || ResolverConfig {
		serve_stale: Some(Duration::from_secs(60)),
		..ResolverConfig::default()
	};

	let inside = with_stale_entry(config(), "inside.test", Duration::from_secs(5));
	assert!(inside.served_stale("inside.test"));

	let outside = with_stale_entry(config(), "outside.test", Duration::from_secs(120));
	assert!(
		!outside.served_stale("outside.test"),
		"an entry past the window is resolved for real, so its address is confirmed"
	);

	let absent = FaithResolver::new(config());
	assert!(!absent.served_stale("absent.test"));
}

#[test]
fn a_network_change_drops_stale_answers() {
	// Addresses read off the old network are exactly what must not be served on the new one
	// (spec:NETCHG#reach-across-the-subsystems).
	let resolver = with_stale_entry(
		ResolverConfig::default(),
		"netchg.test",
		Duration::from_secs(5),
	);
	assert!(resolver.served_stale("netchg.test"));

	resolver.reset();

	assert!(
		!resolver.served_stale("netchg.test"),
		"the stale answer goes with the generation that held it"
	);
}
