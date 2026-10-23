//! CRLite from a upki cache, through an agent. A test binary of its own, as the revocation data
//! is process-wide.

// spec:REVOC#the-systems-crlite-data spec:OBS

#![cfg(all(feature = "unstable-internals", feature = "crlite", feature = "upki"))]

mod common;

use std::time::{Duration, SystemTime};

use common::{serve, wait_for};
use web_faith::{
	Agent,
	agent::{AgentStats, RevocationProvider},
};
use web_faith_revocation::{
	UpkiLocation,
	test_support::{
		FakeRemoteSettings, SigningPki, TlsPki, crlite_filter, crlite_record, write_upki_cache,
	},
};

/// Whether the process's upki cache is fresh or has gone stale, which `install_revocation` fixes
/// for the process.
fn setup(upki_age: Duration, upki_max_age: Duration) -> (TlsPki, tempfile::TempDir, SystemTime) {
	let signing = SigningPki::new();
	let remote = FakeRemoteSettings::start(&signing);
	let pki = TlsPki::new(true);

	let key = pki.intermediate_spki_hash;
	let serial = pki.leaf_serial.clone();
	// upki says revoked and Faith's own data says clean, so whichever answers shows.
	let revoked = crlite_filter(&[(key, serial.clone())], &[]);
	let clean = crlite_filter(&[(key, vec![0x7f])], &[(key, serial)]);
	remote.attach("full", clean.clone());
	remote.publish(
		&signing,
		"cert-revocations",
		vec![crlite_record("full", &clean, "full", false)],
		10,
	);

	let upki = tempfile::tempdir().expect("temp dir");
	let fetched = SystemTime::now() - upki_age;
	write_upki_cache(
		upki.path(),
		&[("20261017-1-default.filter", &revoked)],
		fetched,
	);

	let cache = tempfile::tempdir().expect("temp dir");
	let mut config = signing.config(&remote, Some(cache.path().to_owned()));
	config.upki = UpkiLocation::Dir(upki.path().to_owned());
	config.upki_max_age = upki_max_age;
	assert!(web_faith::install_revocation(config));

	// Both servers must outlive the test.
	std::mem::forget(remote);
	std::mem::forget(cache);
	(pki, upki, fetched)
}

fn agent(pki: &TlsPki, upki: bool) -> Agent {
	Agent::builder()
		.tls(|tls| {
			tls.extra_roots([pki.root_pem.clone()])
				.revocation(|revocation| revocation.onecrl(false).upki(upki).build())
				.build()
		})
		.timeout(|timeout| timeout.total(Duration::from_secs(10)).build())
		.build()
		.expect("the options are valid")
}

fn crlite(stats: &AgentStats) -> web_faith::agent::RevocationStats {
	stats.crlite.expect("CRLite is on")
}

fn secs(time: Option<SystemTime>) -> Option<u64> {
	time.and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
		.map(|since| since.as_secs())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_upki_cache_decides_for_agents_that_use_it() {
	let (pki, _upki, fetched) = setup(Duration::from_secs(60), Duration::from_secs(3 * 86_400));
	let url = format!("https://localhost:{}/", serve(&pki));

	let on_upki = agent(&pki, true);
	let err = on_upki
		.fetch(url.as_str())
		.await
		.expect_err("revoked in upki");
	assert!(
		err.to_string().contains("certificate revoked (CRLite)"),
		"{err}"
	);
	let stats = crlite(&on_upki.stats());
	assert_eq!(stats.provider, Some(RevocationProvider::Upki));
	assert_eq!(secs(stats.last_confirmed), secs(Some(fetched)));
	assert!(stats.refused >= 1);

	// An agent that ignores upki has Faith's own data, which is fetched for it.
	let own = agent(&pki, false);
	wait_for("Faith's own CRLite", || {
		crlite(&own.stats()).provider == Some(RevocationProvider::RemoteSettings)
	});
	assert_eq!(
		own.fetch(url.as_str())
			.await
			.expect("clean in its data")
			.status(),
		200
	);
	assert_eq!(
		crlite(&own.stats()).provider,
		Some(RevocationProvider::RemoteSettings)
	);

	// Neither choice changes the other's.
	assert_eq!(
		crlite(&on_upki.stats()).provider,
		Some(RevocationProvider::Upki)
	);
}
