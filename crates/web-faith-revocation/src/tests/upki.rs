//! CRLite from the system's upki cache and from Faith's own fetch, and the choice between them.

use std::path::Path;

use super::*;
use crate::test_support::{
	set_upki_manifest_time, write_upki_cache, write_upki_index, write_upki_manifest,
};

const FILTER: &str = "20261017-1-default.filter";

/// A host with a upki cache directory of its own, a fake Remote Settings server, and a TLS chain
/// whose leaf the filters are about.
struct Host {
	setup: Setup,
	dir: tempfile::TempDir,
}

impl Host {
	fn new() -> Self {
		Self {
			setup: Setup::new(true),
			dir: tempfile::tempdir().expect("temp dir"),
		}
	}

	fn path(&self) -> &Path {
		self.dir.path()
	}

	/// A filter that lists the leaf as revoked.
	fn revoked(&self) -> Vec<u8> {
		let key = self.setup.tls.intermediate_spki_hash;
		crlite_filter(&[(key, self.setup.tls.leaf_serial.clone())], &[])
	}

	/// A filter that states the leaf is not revoked.
	fn clean(&self) -> Vec<u8> {
		let key = self.setup.tls.intermediate_spki_hash;
		crlite_filter(&[(key, vec![0x7f])], &[(key, self.setup.tls.leaf_serial.clone())])
	}

	/// Lay down a upki cache holding `filter`, last fetched `age` ago.
	fn upki(&self, filter: &[u8], age: Duration) {
		write_upki_cache(self.path(), &[(FILTER, filter)], SystemTime::now() - age);
	}

	fn publish_own(&self, filter: Vec<u8>, timestamp: u64) {
		self.setup
			.publish_crlite(&[("full", filter, false)], timestamp);
	}

	fn revocation(&self, max_age: Duration) -> Revocation {
		let mut config = self.setup.signing.config(&self.setup.server, None);
		config.upki = UpkiLocation::Dir(self.path().to_owned());
		config.upki_max_age = max_age;
		config.upki_poll_interval = Duration::from_millis(20);
		Revocation::new(config, reqwest::Client::new())
	}

	fn client(&self, revocation: &Revocation, sources: Sources) -> Arc<ClientConfig> {
		self.setup
			.client(revocation, sources, &Arc::new(Refusals::default()))
	}

	fn verdict(&self, revocation: &Revocation, sources: Sources) -> Option<Source> {
		revoked_by(handshake(
			&self.client(revocation, sources),
			&self.setup.server_config(),
		))
	}

	fn crlite_requests(&self) -> usize {
		self.setup
			.server
			.requests()
			.iter()
			.filter(|path| path.contains("/collections/cert-revocations/"))
			.count()
	}
}

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn crlite_only(upki: bool) -> Sources {
	Sources {
		onecrl: false,
		upki,
		..Sources::all()
	}
}

fn provider_of(revocation: &Revocation, sources: Sources) -> Option<Provider> {
	revocation
		.in_force(Source::Crlite, sources)
		.map(|in_force| in_force.provider)
}

fn epoch(revocation: &Revocation) -> u64 {
	revocation.inner.store.current_epoch()
}

fn secs_of(time: Option<SystemTime>) -> Option<u64> {
	time.and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
		.map(|since| since.as_secs())
}

#[test]
fn a_fresh_cache_is_the_only_crlite_data_in_force() {
	let host = Host::new();
	host.upki(&host.revoked(), Duration::from_secs(60));
	// Faith's own data says the opposite, so which one decides shows.
	host.publish_own(host.clean(), 10);
	host.setup
		.publish_onecrl(vec![onecrl_issuer_serial(&TlsPki::new(false).intermediate)], 10);
	let revocation = host.revocation(3 * DAY);

	revocation.enable(Sources::all());
	wait_for("OneCRL", || {
		held_timestamp(&revocation, Source::OneCrl) == Some(10)
	});

	assert_eq!(host.verdict(&revocation, Sources::all()), Some(Source::Crlite));
	let in_force = revocation
		.in_force(Source::Crlite, Sources::all())
		.expect("data");
	assert_eq!(in_force.provider, Provider::Upki);
	let manifest = std::fs::metadata(host.path().join("manifest.json"))
		.and_then(|meta| meta.modified())
		.expect("manifest time");
	assert_eq!(in_force.confirmed, Some(manifest));
	assert_eq!(host.crlite_requests(), 0, "the poller did not fetch CRLite");
}

#[test]
fn an_agent_with_upki_off_has_faiths_own_data_alone() {
	let host = Host::new();
	host.upki(&host.revoked(), Duration::from_secs(60));
	host.publish_own(host.clean(), 10);
	let revocation = host.revocation(3 * DAY);

	revocation.enable(crlite_only(true));
	revocation.enable(crlite_only(false));
	wait_for("Faith's own CRLite", || {
		held_timestamp(&revocation, Source::Crlite) == Some(10)
	});

	assert_eq!(host.verdict(&revocation, crlite_only(false)), None);
	assert_eq!(
		provider_of(&revocation, crlite_only(false)),
		Some(Provider::RemoteSettings)
	);
	// The agent on upki is unaffected.
	assert_eq!(host.verdict(&revocation, crlite_only(true)), Some(Source::Crlite));
	assert_eq!(provider_of(&revocation, crlite_only(true)), Some(Provider::Upki));
}

#[test]
fn without_a_cache_faiths_own_data_is_used() {
	let host = Host::new();
	host.publish_own(host.revoked(), 10);
	let revocation = host.revocation(3 * DAY);

	revocation.enable(crlite_only(true));
	wait_for("Faith's own CRLite", || {
		held_timestamp(&revocation, Source::Crlite) == Some(10)
	});

	assert_eq!(host.verdict(&revocation, crlite_only(true)), Some(Source::Crlite));
	assert_eq!(
		provider_of(&revocation, crlite_only(true)),
		Some(Provider::RemoteSettings)
	);
}

#[test]
fn a_stale_cache_stays_in_force_until_faiths_own_data_is_newer() {
	let host = Host::new();
	host.upki(&host.revoked(), 4 * DAY);
	let revocation = host.revocation(3 * DAY);

	revocation.enable(crlite_only(true));
	assert_eq!(host.verdict(&revocation, crlite_only(true)), Some(Source::Crlite));
	assert_eq!(provider_of(&revocation, crlite_only(true)), Some(Provider::Upki));
	assert!(
		!revocation.inner.store.upki_fresh(),
		"four days is past three"
	);

	// Faith fetches its own meanwhile, and once that lands it is the newer.
	wait_for("the poller to try", || host.crlite_requests() > 0);
	host.publish_own(host.clean(), 10);
	wait_for("Faith's own CRLite", || {
		held_timestamp(&revocation, Source::Crlite) == Some(10)
	});
	assert_eq!(host.verdict(&revocation, crlite_only(true)), None);
	assert_eq!(
		provider_of(&revocation, crlite_only(true)),
		Some(Provider::RemoteSettings)
	);
}

#[test]
fn a_stale_cache_newer_than_faiths_own_data_stays_in_force() {
	let host = Host::new();
	host.publish_own(host.clean(), 10);
	let revocation = host.revocation(Duration::from_millis(600));
	revocation.enable(crlite_only(false));
	wait_for("Faith's own CRLite", || {
		held_timestamp(&revocation, Source::Crlite) == Some(10)
	});

	// upki fetched after Faith did, but long enough ago to count as stale by now.
	host.upki(&host.revoked(), Duration::ZERO);
	revocation.enable(crlite_only(true));
	wait_for("the cache to go stale", || {
		!revocation.inner.store.upki_fresh()
	});

	assert_eq!(provider_of(&revocation, crlite_only(true)), Some(Provider::Upki));
	assert_eq!(host.verdict(&revocation, crlite_only(true)), Some(Source::Crlite));
}

#[test]
fn a_refreshed_cache_takes_over_again() {
	let host = Host::new();
	host.upki(&host.revoked(), 4 * DAY);
	host.publish_own(host.clean(), 10);
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	wait_for("Faith's own CRLite", || {
		provider_of(&revocation, crlite_only(true)) == Some(Provider::RemoteSettings)
	});

	set_upki_manifest_time(host.path(), SystemTime::now());
	wait_for("upki to take over", || {
		provider_of(&revocation, crlite_only(true)) == Some(Provider::Upki)
	});
	assert_eq!(host.verdict(&revocation, crlite_only(true)), Some(Source::Crlite));
}

#[test]
fn a_manifest_rewritten_with_new_filters_is_enforced_without_a_restart() {
	let host = Host::new();
	host.upki(&host.clean(), Duration::from_secs(60));
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	let client = host.client(&revocation, crlite_only(true));
	let server = host.setup.server_config();
	assert!(handshake(&client, &server).is_ok());

	let revoked = host.revoked();
	write_upki_cache(host.path(), &[("20261018-0-default.filter", &revoked)], SystemTime::now());
	wait_for("the new filter", || {
		revoked_by(handshake(&client, &server)) == Some(Source::Crlite)
	});
}

#[test]
fn a_cache_that_appears_while_the_process_runs_is_found() {
	let host = Host::new();
	host.publish_own(host.clean(), 10);
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	wait_for("Faith's own CRLite", || {
		provider_of(&revocation, crlite_only(true)) == Some(Provider::RemoteSettings)
	});

	host.upki(&host.revoked(), Duration::from_secs(60));
	wait_for("the cache", || {
		provider_of(&revocation, crlite_only(true)) == Some(Provider::Upki)
	});
	assert_eq!(host.verdict(&revocation, crlite_only(true)), Some(Source::Crlite));
}

#[test]
fn a_reload_that_finds_the_cache_unusable_keeps_the_data_held() {
	let host = Host::new();
	host.upki(&host.revoked(), Duration::from_secs(120));
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	let before = revocation.in_force(Source::Crlite, crlite_only(true));
	let epoch_before = epoch(&revocation);

	std::fs::write(host.path().join(FILTER), b"damaged").expect("damaging");
	set_upki_manifest_time(host.path(), SystemTime::now());
	revocation.inner.store.refresh_upki();

	assert_eq!(revocation.in_force(Source::Crlite, crlite_only(true)), before);
	assert_eq!(host.verdict(&revocation, crlite_only(true)), Some(Source::Crlite));
	assert_eq!(epoch(&revocation), epoch_before);
}

#[test]
fn an_unchanged_filter_set_only_moves_the_confirmed_time() {
	let host = Host::new();
	let revoked = host.revoked();
	host.upki(&revoked, Duration::from_secs(600));
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	let first = revocation
		.in_force(Source::Crlite, crlite_only(true))
		.expect("data");
	let epoch_before = epoch(&revocation);

	set_upki_manifest_time(host.path(), SystemTime::now());
	revocation.inner.store.refresh_upki();

	let second = revocation
		.in_force(Source::Crlite, crlite_only(true))
		.expect("data");
	assert!(second.confirmed > first.confirmed);
	assert_eq!(epoch(&revocation), epoch_before, "sessions stay resumable");
}

#[test]
fn a_changed_filter_set_moves_the_epoch() {
	let host = Host::new();
	host.upki(&host.clean(), Duration::from_secs(600));
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	let epoch_before = epoch(&revocation);

	let revoked = host.revoked();
	write_upki_cache(host.path(), &[(FILTER, &revoked)], SystemTime::now());
	revocation.inner.store.refresh_upki();
	assert!(epoch(&revocation) > epoch_before);
}

#[test]
fn freshness_flipping_either_way_moves_the_epoch() {
	let host = Host::new();
	host.upki(&host.revoked(), Duration::from_secs(60));
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	let fresh = epoch(&revocation);

	set_upki_manifest_time(host.path(), SystemTime::now() - 4 * DAY);
	revocation.inner.store.refresh_upki();
	let stale = epoch(&revocation);
	assert!(stale > fresh);

	set_upki_manifest_time(host.path(), SystemTime::now());
	revocation.inner.store.refresh_upki();
	assert!(epoch(&revocation) > stale);
	assert!(revocation.inner.store.upki_fresh());

	// Nothing changed: nothing moves.
	let settled = epoch(&revocation);
	revocation.inner.store.refresh_upki();
	assert_eq!(epoch(&revocation), settled);
}

#[test]
fn a_session_saved_under_the_other_provider_is_not_resumed() {
	let host = Host::new();
	host.upki(&host.clean(), Duration::from_secs(60));
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	let client = host.client(&revocation, crlite_only(true));
	let server = host.setup.server_config();
	assert_eq!(handshake(&client, &server), Ok(HandshakeKind::Full));
	assert_eq!(handshake(&client, &server), Ok(HandshakeKind::Resumed));

	let revoked = host.revoked();
	write_upki_cache(host.path(), &[(FILTER, &revoked)], SystemTime::now());
	revocation.inner.store.refresh_upki();
	assert_eq!(revoked_by(handshake(&client, &server)), Some(Source::Crlite));
}

#[test]
fn the_confirmed_time_is_the_manifests_modification_time() {
	let host = Host::new();
	let at = SystemTime::now() - Duration::from_secs(3600);
	write_upki_cache(host.path(), &[(FILTER, &host.clean())], at);
	let revocation = host.revocation(3 * DAY);
	revocation.enable(crlite_only(true));
	assert_eq!(
		secs_of(
			revocation
				.in_force(Source::Crlite, crlite_only(true))
				.and_then(|in_force| in_force.confirmed)
		),
		secs_of(Some(at))
	);
}

#[test]
fn a_cache_with_a_damaged_manifest_is_not_used() {
	let host = Host::new();
	host.upki(&host.revoked(), Duration::from_secs(60));
	write_upki_manifest(host.path(), &[], SystemTime::now());
	host.publish_own(host.clean(), 10);
	let revocation = host.revocation(3 * DAY);

	revocation.enable(crlite_only(true));
	wait_for("Faith's own CRLite", || {
		held_timestamp(&revocation, Source::Crlite) == Some(10)
	});
	assert_eq!(
		provider_of(&revocation, crlite_only(true)),
		Some(Provider::RemoteSettings)
	);
	let _ = write_upki_index;
}
