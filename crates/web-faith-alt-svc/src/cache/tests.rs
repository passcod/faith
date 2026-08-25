use std::time::{Duration, Instant};

use super::*;

#[test]
fn test_ipv6_origin_accepts_its_own_host_spelled_out() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://[2001:db8::1]/path").unwrap();

	cache.record_alt_svc(
		&url,
		&AltSvcAdvertisement {
			host: "[2001:db8::1]".to_string(),
			port: 443,
			max_age: None,
		},
	);

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"an IPv6 origin naming its own address is the same host, brackets and all"
	);
}

#[test]
fn test_host_comparison_ignores_case() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(
		&url,
		&AltSvcAdvertisement {
			host: "ExAmPlE.CoM".to_string(),
			port: 443,
			max_age: None,
		},
	);

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"host names are case-insensitive, so this still names the origin's own host"
	);
}

#[test]
fn test_alt_svc_on_another_host_is_not_recorded() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(
		&url,
		&AltSvcAdvertisement {
			host: "cdn.example.net".to_string(),
			port: 443,
			max_age: None,
		},
	);

	assert!(
		cache.should_use_h3(&url).is_none(),
		"h3 on another host says nothing about this one, and the port matching is \
		 coincidental"
	);
}

#[test]
fn test_alt_svc_naming_our_own_host_is_recorded() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(
		&url,
		&AltSvcAdvertisement {
			host: "example.com".to_string(),
			port: 443,
			max_age: None,
		},
	);

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"spelling out the origin's own host is equivalent to omitting it"
	);
}

#[test]
fn test_confirm_h3_uses_the_port_it_was_given() {
	// A concurrent failure can clear both caches between the attempt starting and
	// confirming. `confirm_h3` must not fall back to the origin's port then, or it
	// would confirm h3 on a port nobody advertised.
	let cache = test_cache_with(3, Duration::from_secs(60), true);
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(8443, None));
	cache.record_h3_failure(&url);
	cache.confirm_h3(&url, 8443);

	let entry = cache
		.confirmed
		.get(&"https://example.com:443".to_string())
		.expect("the successful attempt is confirmed");
	assert_eq!(
		entry.port, 8443,
		"confirmed on the port actually connected to, not the origin's"
	);
}

/// A same-host advertisement, the common case.
fn ad(port: u16, max_age: Option<Duration>) -> AltSvcAdvertisement {
	AltSvcAdvertisement {
		host: String::new(),
		port,
		max_age,
	}
}

fn test_cache() -> AltSvcCache {
	test_cache_with(3, Duration::from_secs(60), false)
}

fn test_cache_with(
	cancel_strikes: u32,
	strike_window: Duration,
	follow_advertised_port: bool,
) -> AltSvcCache {
	test_cache_failing(
		cancel_strikes,
		strike_window,
		follow_advertised_port,
		Duration::from_secs(300),
		Duration::from_secs(3600),
	)
}

/// A cache whose knowledge expires soon, for tests about knowledge that has lapsed.
fn test_cache_ttls(advertised_ttl: Duration, confirmed_ttl: Duration) -> AltSvcCache {
	AltSvcCache::new(AltSvcCacheConfig {
		advertised_ttl,
		confirmed_ttl,
		failed_ttl: Duration::from_secs(300),
		failed_max_ttl: Duration::from_secs(3600),
		capacity: 10_000,
		cancel_strikes: 3,
		strike_window: Duration::from_secs(60),
		follow_advertised_port: false,
		probe_ttl: Duration::from_secs(10),
		slow_factor: 2.5,
		slow_ttl: Duration::from_millis(200),
	})
}

fn test_cache_failing(
	cancel_strikes: u32,
	strike_window: Duration,
	follow_advertised_port: bool,
	failed_ttl: Duration,
	failed_max_ttl: Duration,
) -> AltSvcCache {
	AltSvcCache::new(AltSvcCacheConfig {
		advertised_ttl: Duration::from_secs(86400),
		confirmed_ttl: Duration::from_secs(86400),
		failed_ttl,
		failed_max_ttl,
		capacity: 10_000,
		cancel_strikes,
		strike_window,
		follow_advertised_port,
		probe_ttl: Duration::from_secs(10),
		slow_factor: 2.5,
		slow_ttl: Duration::from_millis(200),
	})
}

#[test]
fn test_advertised_port_matching_origin_upgrades() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"an advertisement for the origin's own port is actionable"
	);
}

#[test]
fn test_advertised_port_mismatch_does_not_upgrade() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(8443, None));

	assert!(
		cache.should_use_h3(&url).is_none(),
		"h3 advertised on :8443 says nothing about :443, so don't upgrade"
	);
}

#[test]
fn test_advertised_port_mismatch_is_still_recorded() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(8443, None));

	let entry = cache
		.advertised
		.get(&"https://example.com:443".to_string())
		.expect("the advertisement is kept even though it isn't actionable");
	assert_eq!(
		entry.port, 8443,
		"keeping it means the port is available if reqwest ever lets us honour it"
	);
}

#[test]
fn test_advertised_port_mismatch_upgrades_when_following() {
	let cache = test_cache_with(3, Duration::from_secs(60), true);
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(8443, None));

	assert_eq!(
		cache.should_use_h3(&url),
		Some(8443),
		"opting in returns the advertised port so the request can be rewritten"
	);
}

#[test]
fn test_cache_flow() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	assert!(cache.should_use_h3(&url).is_none());

	cache.record_alt_svc(&url, &ad(443, Some(Duration::from_secs(3600))));
	assert_eq!(cache.should_use_h3(&url), Some(443));

	cache.confirm_h3(&url, 443);
	assert_eq!(cache.should_use_h3(&url), Some(443));
	assert!(
		!cache
			.advertised
			.contains_key(&"https://example.com:443".to_string())
	);
	assert!(
		cache
			.confirmed
			.contains_key(&"https://example.com:443".to_string())
	);
}

#[test]
fn test_cache_failure() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	assert!(cache.should_use_h3(&url).is_some());

	cache.record_h3_failure(&url);
	assert!(cache.should_use_h3(&url).is_none());

	cache.record_alt_svc(&url, &ad(443, None));
	assert!(cache.should_use_h3(&url).is_none());
}

#[test]
fn test_hint() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.add_hint("example.com", 443);
	assert_eq!(cache.should_use_h3(&url), Some(443));
}

#[test]
fn test_hint_is_confirmed_not_probed() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.add_hint("example.com", 443);

	assert_eq!(
		cache.confirmed_port(&url),
		Some(443),
		"a hint is the user's assertion and routes immediately, probe or no probe"
	);
	assert!(
		cache.probe_candidate(&url).is_none(),
		"nothing to verify: the hint already confirmed the origin"
	);
}

#[test]
fn test_https_record_lands_as_an_advertisement_not_a_confirmation() {
	// spec:H3UP#advertisements-from-dns — DNS is a second source of the same advertisement,
	// so it makes the origin probe-worthy without routing a foreground request onto an
	// unverified QUIC path.
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_https_record(&url, None, Duration::from_secs(3600));

	assert!(
		cache.confirmed_port(&url).is_none(),
		"a record is evidence worth probing, not worth routing on"
	);
	assert_eq!(
		cache.probe_candidate(&url),
		Some(443),
		"and a record naming no port describes the origin's own"
	);
}

#[test]
fn test_https_record_port_follows_the_advertised_port_rules() {
	// spec:H3UP#advertised-ports — a `port` differing from the origin's is treated exactly as
	// an `Alt-Svc` advertised port: recorded, but not acted on by default.
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_https_record(&url, Some(8443), Duration::from_secs(3600));

	assert!(
		cache.probe_candidate(&url).is_none(),
		"h3 on :8443 says nothing about :443, so nothing is probed"
	);

	let following = test_cache_with(3, Duration::from_secs(60), true);
	following.record_https_record(&url, Some(8443), Duration::from_secs(3600));
	assert_eq!(
		following.probe_candidate(&url),
		Some(8443),
		"opting into the quirk probes the advertised port"
	);
}

#[test]
fn test_https_record_is_refused_while_the_origin_is_failed() {
	// A failure blocks recording fresh advertisements whatever their source, or a flapping
	// origin could re-enter the cycle through DNS (spec:H3UP#advertisements-from-dns).
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_h3_failure(&url);
	cache.record_https_record(&url, None, Duration::from_secs(3600));

	assert!(cache.probe_candidate(&url).is_none());
	assert!(
		!cache.wants_https_record(&url),
		"and there is no point querying again while the cooldown runs"
	);
}

#[test]
fn test_wants_https_record_only_while_there_is_something_to_learn() {
	// spec:DNS#https-records — the gate is what keeps the query off every lookup once the
	// origin's HTTP/3 support is settled either way.
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	let unknown = test_cache();
	assert!(
		unknown.wants_https_record(&url),
		"an origin nothing is known about is worth asking about"
	);

	let advertised = test_cache();
	advertised.record_alt_svc(&url, &ad(443, None));
	assert!(
		!advertised.wants_https_record(&url),
		"a live advertisement already warrants the probe a record would"
	);

	let confirmed = test_cache();
	confirmed.confirm_h3(&url, 443);
	assert!(
		!confirmed.wants_https_record(&url),
		"a confirmed origin is already routing over HTTP/3"
	);
}

#[test]
fn test_wants_https_record_again_once_the_advertisement_lapses() {
	// The gate must not be permanent: an advertisement that expires without being confirmed
	// leaves the origin unknown again, and DNS is how it can be re-learned.
	let cache = test_cache_ttls(Duration::from_millis(50), Duration::from_secs(86400));
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	assert!(!cache.wants_https_record(&url));

	std::thread::sleep(Duration::from_millis(120));

	assert!(
		cache.wants_https_record(&url),
		"once the advertisement lapses the query is worth making again"
	);
}

#[test]
fn test_https_record_ttl_bounds_the_advertisement() {
	// spec:H3UP#advertisements-from-dns — the record's own DNS TTL is what the advertisement
	// lives for, the role `ma` plays for a header.
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_https_record(&url, None, Duration::from_millis(50));
	assert_eq!(cache.probe_candidate(&url), Some(443));

	std::thread::sleep(Duration::from_millis(120));

	assert!(
		cache.probe_candidate(&url).is_none(),
		"past the record's TTL the advertisement is no longer evidence"
	);
}

#[test]
fn test_advertised_routes_nothing_but_probes() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));

	assert!(
		cache.confirmed_port(&url).is_none(),
		"an advertisement is evidence worth probing, not worth routing on"
	);
	assert_eq!(
		cache.probe_candidate(&url),
		Some(443),
		"and it is exactly what the probe should verify"
	);
}

#[test]
fn test_probe_candidate_respects_failed() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.record_h3_failure(&url);

	assert!(
		cache.probe_candidate(&url).is_none(),
		"a failed origin is not re-probed until the cooldown lapses"
	);
}

#[test]
fn test_probe_confirmation_promotes() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	assert!(cache.claim_probe(&url), "first claim wins");
	cache.confirm_h3(&url, 443);
	cache.finish_probe(&url);

	assert_eq!(cache.confirmed_port(&url), Some(443));
	assert!(
		cache.probe_candidate(&url).is_none(),
		"confirmed origins are not probed again"
	);
}

#[test]
fn test_claim_probe_is_single_flight() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	assert!(cache.claim_probe(&url));
	assert!(
		!cache.claim_probe(&url),
		"a second claim while one is in flight loses"
	);

	cache.finish_probe(&url);
	assert!(
		cache.claim_probe(&url),
		"finishing the probe frees the origin for the next one"
	);
}

#[test]
fn test_slow_demotion_needs_sustained_evidence() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.confirm_h3(&url, 443);

	// Plenty of TCP samples at 5ms, but too few QUIC samples to act on.
	for _ in 0..EWMA_MIN_SAMPLES {
		cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(5));
	}
	for _ in 0..(EWMA_MIN_SAMPLES - 1) {
		cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(50));
	}

	assert_eq!(
		cache.confirmed_port(&url),
		Some(443),
		"below the minimum sample count no comparison may act"
	);
}

#[test]
fn test_slow_demotion_moves_origin_back_to_probing() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.confirm_h3(&url, 443);

	// TCP steady at 5ms, QUIC steady at 50ms: 10x the average and 45ms over,
	// clearing both the factor and the absolute floor.
	for _ in 0..EWMA_MIN_SAMPLES {
		cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(5));
		cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(50));
	}

	assert!(
		cache.confirmed_port(&url).is_none(),
		"a sustained large gap demotes the origin off HTTP/3"
	);
	assert!(
		cache.probe_candidate(&url).is_none(),
		"while the slow marker lives, the origin is not re-probed either"
	);
	assert!(
		!cache.is_failed("https://example.com:443"),
		"slow is not broken: the failed cache stays out of it"
	);

	// The test cache's slow TTL is short; once it lapses, the advertisement
	// preserved by the demotion re-enters through a probe.
	std::thread::sleep(Duration::from_millis(300));
	assert_eq!(
		cache.probe_candidate(&url),
		Some(443),
		"slow expiry re-enters via the probe, asking whether the path improved"
	);
}

#[test]
fn test_parity_or_moderately_slower_quic_is_kept() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.confirm_h3(&url, 443);

	// QUIC 2x slower and 20ms over: above the floor but below the 2.5x
	// factor, so HTTP/3's structural advantages win the tie.
	for _ in 0..(EWMA_MIN_SAMPLES * 2) {
		cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(20));
		cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(40));
	}

	assert_eq!(
		cache.confirmed_port(&url),
		Some(443),
		"moderately slower QUIC is still preferred"
	);
}

#[test]
fn test_cancellation_below_threshold_keeps_h3() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();
	cache.record_alt_svc(&url, &ad(443, None));

	cache.record_h3_cancellation(&url);
	cache.record_h3_cancellation(&url);

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"two strikes is not enough to demote"
	);
}

#[test]
fn test_cancellation_at_threshold_demotes() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();
	cache.record_alt_svc(&url, &ad(443, None));

	for _ in 0..3 {
		cache.record_h3_cancellation(&url);
	}

	assert!(
		cache.should_use_h3(&url).is_none(),
		"three strikes demotes the origin"
	);
	assert!(
		cache.is_failed("https://example.com:443"),
		"demotion goes through the failed cache, so re-advertisement can't re-arm it"
	);
}

#[test]
fn test_cancellation_reset_by_h3_success() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();
	cache.record_alt_svc(&url, &ad(443, None));

	cache.record_h3_cancellation(&url);
	cache.record_h3_cancellation(&url);
	cache.confirm_h3(&url, 443);
	cache.record_h3_cancellation(&url);
	cache.record_h3_cancellation(&url);

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"a working h3 response clears the strikes, so these two start over"
	);
}

#[test]
fn test_cancellation_disabled_by_zero() {
	let cache = test_cache_with(0, Duration::from_secs(60), false);
	let url = reqwest::Url::parse("https://example.com/path").unwrap();
	cache.record_alt_svc(&url, &ad(443, None));

	for _ in 0..5 {
		cache.record_h3_cancellation(&url);
	}

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"cancel_strikes: 0 disables cancellation-based demotion"
	);
}

#[test]
fn test_cancellation_strikes_decay() {
	let cache = test_cache_with(3, Duration::from_millis(50), false);
	let url = reqwest::Url::parse("https://example.com/path").unwrap();
	cache.record_alt_svc(&url, &ad(443, None));

	cache.record_h3_cancellation(&url);
	cache.record_h3_cancellation(&url);
	std::thread::sleep(Duration::from_millis(150));
	cache.record_h3_cancellation(&url);
	cache.record_h3_cancellation(&url);

	assert_eq!(
		cache.should_use_h3(&url),
		Some(443),
		"strikes older than the window don't count towards the run"
	);
}

fn failure_entry(cache: &AltSvcCache) -> FailureEntry {
	cache
		.failed
		.get("https://example.com:443")
		.expect("the origin has a failure on record")
}

#[test]
fn test_failure_cooldown_doubles_up_to_the_cap() {
	let cache = test_cache();

	let schedule: Vec<u64> = (1..=6)
		.map(|count| cache.failure_cooldown(count).as_secs())
		.collect();

	assert_eq!(
		schedule,
		vec![300, 600, 1200, 2400, 3600, 3600],
		"each consecutive failure doubles the base, then holds at the cap"
	);
}

#[test]
fn test_failure_cooldown_cap_below_base_is_flat() {
	let cache = test_cache_failing(
		3,
		Duration::from_secs(60),
		false,
		Duration::from_secs(300),
		Duration::from_secs(60),
	);

	let schedule: Vec<u64> = (1..=4)
		.map(|count| cache.failure_cooldown(count).as_secs())
		.collect();

	assert_eq!(
		schedule,
		vec![300, 300, 300, 300],
		"a cap under the base is clamped up to it, giving a cooldown that never backs off"
	);
}

#[test]
fn test_consecutive_failures_lengthen_the_cooldown() {
	let cache = test_cache_failing(
		3,
		Duration::from_secs(60),
		false,
		Duration::from_millis(200),
		Duration::from_secs(60),
	);
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.record_h3_failure(&url);
	assert!(
		cache.is_failed("https://example.com:443"),
		"the first failure blocks the origin"
	);

	// Past the first cooldown, but well inside the run's own lifetime: this is
	// the retry the cooldown allowed, and it fails too.
	std::thread::sleep(Duration::from_millis(250));
	assert!(
		!cache.is_failed("https://example.com:443"),
		"the first cooldown lapses on its own"
	);

	cache.record_h3_failure(&url);
	let entry = failure_entry(&cache);
	assert_eq!(
		entry.count, 2,
		"failing again straight after a lapsed cooldown continues the run"
	);
	assert!(
		cache.is_failed("https://example.com:443"),
		"and blocks the origin again, for twice as long"
	);
}

#[test]
fn test_run_lapses_when_the_origin_stops_failing() {
	let cache = test_cache_failing(
		3,
		Duration::from_secs(60),
		false,
		Duration::from_millis(100),
		Duration::from_secs(60),
	);
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_h3_failure(&url);
	// One cooldown beyond the block it caused: nobody exercised the origin in
	// that stretch, so the next failure is judged on its own.
	std::thread::sleep(Duration::from_millis(300));

	cache.record_h3_failure(&url);
	assert_eq!(
		failure_entry(&cache).count,
		1,
		"an origin left alone past its run starts from the base cooldown again"
	);
}

#[test]
fn test_confirmation_ends_the_run() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_h3_failure(&url);
	cache.record_h3_failure(&url);
	assert_eq!(failure_entry(&cache).count, 2);

	cache.confirm_h3(&url, 443);
	cache.record_h3_failure(&url);

	let entry = failure_entry(&cache);
	assert_eq!(
		entry.count, 1,
		"a working h3 response ends the run, so the next failure starts at the base"
	);
	assert_eq!(
		entry.blocked_until.duration_since(Instant::now()).as_secs(),
		299,
		"and is blocked for the base cooldown, not the doubled one"
	);
}

#[test]
fn test_confirmation_does_not_cut_a_live_cooldown_short() {
	// A confirmation can race a concurrent failure. The failure is the more
	// recent word on the path, so it keeps the origin blocked; only the run
	// is forgotten.
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_h3_failure(&url);
	cache.confirm_h3(&url, 443);

	assert!(
		cache.is_failed("https://example.com:443"),
		"the cooldown the failure set still runs"
	);
	assert_eq!(
		failure_entry(&cache).count,
		0,
		"but the run behind it is cleared"
	);
}

#[test]
fn test_network_change_demotes_confirmed_to_advertised() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.confirm_h3(&url, 443);

	cache.network_changed();

	assert!(
		cache.confirmed_port(&url).is_none(),
		"the path that proved HTTP/3 is gone, so the origin is no longer confirmed"
	);
	assert_eq!(
		cache.probe_candidate(&url),
		Some(443),
		"it keeps its advertisement, so a background probe re-verifies it at once"
	);
}

#[test]
fn test_network_change_clears_failures_and_their_backoff() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.record_h3_failure(&url);
	cache.record_alt_svc(&url, &ad(443, None));

	cache.network_changed();

	assert!(
		!cache.is_failed("https://example.com:443"),
		"a blocked UDP path is a fact about the old network"
	);
	assert!(
		cache.failed.get("https://example.com:443").is_none(),
		"and so is the run of failures that set the cooldown"
	);

	// The advertisement a failure discards has to come back for the origin to be
	// probe-worthy again, so re-record it as a live response would.
	cache.record_alt_svc(&url, &ad(443, None));
	cache.record_h3_failure(&url);
	assert_eq!(
		failure_entry(&cache).count,
		1,
		"failing on the new network starts the backoff from the base cooldown"
	);
}

#[test]
fn test_network_change_clears_a_slow_demotion() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.confirm_h3(&url, 443);
	for _ in 0..EWMA_MIN_SAMPLES {
		cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(5));
		cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(50));
	}
	assert!(
		cache.probe_candidate(&url).is_none(),
		"the slow marker holds the origin off probing before the signal"
	);

	cache.network_changed();

	assert_eq!(
		cache.probe_candidate(&url),
		Some(443),
		"a slow path was slow on the old network, so the origin re-enters through a probe"
	);
}

#[test]
fn test_network_change_clears_the_path_time_averages() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	cache.confirm_h3(&url, 443);
	// Enough TCP samples to satisfy the comparison's minimum, all of them fast.
	for _ in 0..EWMA_MIN_SAMPLES {
		cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(5));
	}

	cache.network_changed();
	cache.confirm_h3(&url, 443);

	// Slow enough to demote several times over, were the old TCP average still there
	// to compare against.
	for _ in 0..EWMA_MIN_SAMPLES {
		cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(50));
	}

	assert_eq!(
		cache.confirmed_port(&url),
		Some(443),
		"with the TCP average cleared there is nothing to judge QUIC against, so \
		 no demotion happens on one side's samples alone"
	);
}

#[test]
fn test_network_change_keeps_hints_confirmed() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.add_hint("example.com", 443);

	cache.network_changed();

	assert_eq!(
		cache.confirmed_port(&url),
		Some(443),
		"a hint is the caller's assertion, not an observation, so it survives the signal \
		 and keeps an HTTP/3-only origin reachable"
	);
	assert!(
		cache.probe_candidate(&url).is_none(),
		"and a hinted origin is still never probed"
	);
}

#[test]
fn test_network_change_lets_a_blocked_hint_take_effect() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.add_hint("example.com", 443);
	cache.record_h3_failure(&url);
	assert!(
		cache.confirmed_port(&url).is_none(),
		"a failure demotes a hinted origin like any other"
	);

	cache.network_changed();

	assert_eq!(
		cache.confirmed_port(&url),
		Some(443),
		"the failure that was masking the hint belonged to the old network, so the \
		 hint holds again once it is cleared"
	);
}

#[test]
fn test_network_change_keeps_advertisements() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));

	cache.network_changed();

	assert_eq!(
		cache.probe_candidate(&url),
		Some(443),
		"an advertisement is the origin's statement about itself, which a change of \
		 client network does not revise"
	);
}

#[test]
fn test_network_change_drops_expired_confirmations() {
	let cache = test_cache_ttls(Duration::from_millis(100), Duration::from_millis(100));
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.confirm_h3(&url, 443);
	std::thread::sleep(Duration::from_millis(150));

	cache.network_changed();

	assert!(
		cache.probe_candidate(&url).is_none(),
		"a confirmation that had already lapsed is not knowledge to carry forward as \
		 a fresh advertisement"
	);
}

#[test]
fn test_network_change_releases_probe_claims() {
	let cache = test_cache();
	let url = reqwest::Url::parse("https://example.com/path").unwrap();

	cache.record_alt_svc(&url, &ad(443, None));
	assert!(cache.claim_probe(&url), "the first probe claims the origin");
	assert!(!cache.claim_probe(&url), "and holds it single-flight");

	cache.network_changed();

	assert!(
		cache.claim_probe(&url),
		"probes in flight are aborted with the client they ran on, so their claims \
		 must not hold the origin for the claim TTL"
	);
}
