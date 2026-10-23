//! A stale upki cache, through an agent: Faith's own fetch takes over once it lands. A test
//! binary of its own, as the revocation data is process-wide.

// spec:REVOC#a-fresh-cache

#![cfg(all(feature = "unstable-internals", feature = "crlite", feature = "upki"))]

mod common;

use std::time::{Duration, SystemTime};

use common::{serve, wait_for};
use web_faith::{Agent, agent::RevocationProvider};
use web_faith_revocation::{
	UpkiLocation,
	test_support::{
		FakeRemoteSettings, SigningPki, TlsPki, crlite_filter, crlite_record, write_upki_cache,
	},
};

#[tokio::test(flavor = "multi_thread")]
async fn a_stale_upki_cache_gives_way_to_faiths_own_data() {
	let signing = SigningPki::new();
	let remote = FakeRemoteSettings::start(&signing);
	let pki = TlsPki::new(true);
	let key = pki.intermediate_spki_hash;
	let serial = pki.leaf_serial.clone();

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
	write_upki_cache(
		upki.path(),
		&[("20261017-1-default.filter", &revoked)],
		SystemTime::now() - Duration::from_secs(4 * 86_400),
	);
	let cache = tempfile::tempdir().expect("temp dir");
	let mut config = signing.config(&remote, Some(cache.path().to_owned()));
	config.upki = UpkiLocation::Dir(upki.path().to_owned());
	assert!(web_faith::install_revocation(config));

	let agent = Agent::builder()
		.tls(|tls| {
			tls.extra_roots([pki.root_pem.clone()])
				.revocation(|revocation| revocation.onecrl(false).build())
				.build()
		})
		.timeout(|timeout| timeout.total(Duration::from_secs(10)).build())
		.build()
		.expect("the options are valid");

	wait_for("Faith's own CRLite to land", || {
		agent.stats().crlite.and_then(|stats| stats.provider)
			== Some(RevocationProvider::RemoteSettings)
	});
	let url = format!("https://localhost:{}/", serve(&pki));
	assert_eq!(
		agent.fetch(url).await.expect("clean in its data").status(),
		200
	);
}
