//! The pieces together: a fake Remote Settings server, the poller, the cache, and real rustls
//! handshakes through the verifier.

use std::{
	sync::Arc,
	time::{Duration, Instant, SystemTime},
};

use rustls::{CertificateError, ClientConfig, HandshakeKind, client::Resumption};
use serde_json::Value;

use super::*;
use crate::test_support::{
	FakeRemoteSettings, SigningPki, TlsPki, crlite_filter, crlite_record, handshake,
	onecrl_issuer_serial, provider, transfer,
};

#[cfg(feature = "upki")]
mod upki;

const POLL: Duration = Duration::from_millis(200);

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
	let deadline = Instant::now() + Duration::from_secs(15);
	while !done() {
		assert!(Instant::now() < deadline, "timed out waiting for {what}");
		std::thread::sleep(Duration::from_millis(20));
	}
}

struct Setup {
	signing: SigningPki,
	server: FakeRemoteSettings,
	tls: TlsPki,
}

impl Setup {
	fn new(with_sct: bool) -> Self {
		let signing = SigningPki::new();
		let server = FakeRemoteSettings::start(&signing);
		Self {
			signing,
			server,
			tls: TlsPki::new(with_sct),
		}
	}

	fn revocation(&self, cache_dir: Option<std::path::PathBuf>, interval: Duration) -> Revocation {
		let mut config = self.signing.config(&self.server, cache_dir);
		config.poll_interval = interval;
		Revocation::new(config, reqwest::Client::new())
	}

	fn publish_onecrl(&self, records: Vec<Value>, timestamp: u64) {
		self.server
			.publish(&self.signing, "onecrl", records, timestamp);
	}

	fn publish_crlite(&self, filters: &[(&str, Vec<u8>, bool)], timestamp: u64) {
		let records = filters
			.iter()
			.map(|(name, bytes, incremental)| {
				self.server.attach(name, bytes.clone());
				crlite_record(name, bytes, name, *incremental)
			})
			.collect();
		self.server
			.publish(&self.signing, "cert-revocations", records, timestamp);
	}

	fn client(
		&self,
		revocation: &Revocation,
		sources: Sources,
		refusals: &Arc<Refusals>,
	) -> Arc<ClientConfig> {
		let verifier = revocation.verifier(
			self.tls.verifier(),
			sources,
			std::slice::from_ref(&self.tls.root),
			Arc::clone(refusals),
		);
		let mut config = ClientConfig::builder_with_provider(provider())
			.with_safe_default_protocol_versions()
			.expect("protocol versions")
			.dangerous()
			.with_custom_certificate_verifier(verifier)
			.with_no_client_auth();
		config.resumption = Resumption::store(revocation.session_store());
		Arc::new(config)
	}

	fn server_config(&self) -> Arc<rustls::ServerConfig> {
		Arc::new(self.tls.server_config())
	}
}

fn revoked_by(result: Result<HandshakeKind, rustls::Error>) -> Option<Source> {
	match result {
		Err(rustls::Error::InvalidCertificate(CertificateError::Other(other))) => other
			.0
			.downcast_ref::<Revoked>()
			.map(|revoked| revoked.source),
		_ => None,
	}
}

/// The sources of an agent that uses `source` alone.
fn only(source: Source) -> Sources {
	let mut sources = Sources::none();
	match source {
		#[cfg(feature = "onecrl")]
		Source::OneCrl => sources.onecrl = true,
		#[cfg(feature = "crlite")]
		Source::Crlite => sources.crlite = true,
	}
	sources
}

/// When `source`'s data was last confirmed current, for an agent that uses every source.
fn last_confirmed(revocation: &Revocation, source: Source) -> Option<SystemTime> {
	revocation
		.in_force(source, Sources::all())
		.and_then(|in_force| in_force.confirmed)
}

fn held_timestamp(revocation: &Revocation, source: Source) -> Option<u64> {
	revocation.inner.store.held(source).0
}

#[test]
fn a_listed_intermediate_is_refused() {
	let setup = Setup::new(false);
	setup.publish_onecrl(vec![onecrl_issuer_serial(&setup.tls.intermediate)], 10);
	let revocation = setup.revocation(None, DEFAULT_POLL_INTERVAL);

	let refusals = Arc::new(Refusals::default());
	let client = setup.client(&revocation, Sources::all(), &refusals);
	let server = setup.server_config();
	assert!(handshake(&client, &server).is_ok(), "nothing is known yet");

	revocation.enable(only(Source::OneCrl));
	wait_for("OneCRL", || {
		last_confirmed(&revocation, Source::OneCrl).is_some()
	});

	let error = handshake(&client, &server);
	assert_eq!(revoked_by(error.clone()), Some(Source::OneCrl), "{error:?}");
	let message = error.expect_err("refused").to_string();
	assert!(
		message.contains("certificate revoked (OneCRL)"),
		"{message}"
	);
	assert_eq!(refusals.count(Source::OneCrl), 1);
	assert_eq!(refusals.count(Source::Crlite), 0);

	let opted_out = setup.client(
		&revocation,
		Sources {
			onecrl: false,
			..Sources::all()
		},
		&refusals,
	);
	assert!(handshake(&opted_out, &server).is_ok());
}

#[test]
fn a_revoked_leaf_is_refused() {
	let setup = Setup::new(true);
	let full = crlite_filter(
		&[(
			setup.tls.intermediate_spki_hash,
			setup.tls.leaf_serial.clone(),
		)],
		&[],
	);
	setup.publish_crlite(&[("full", full, false)], 10);
	let revocation = setup.revocation(None, DEFAULT_POLL_INTERVAL);
	revocation.enable(only(Source::Crlite));
	wait_for("CRLite", || {
		last_confirmed(&revocation, Source::Crlite).is_some()
	});

	let refusals = Arc::new(Refusals::default());
	let server = setup.server_config();
	let client = setup.client(&revocation, Sources::all(), &refusals);
	assert_eq!(
		revoked_by(handshake(&client, &server)),
		Some(Source::Crlite)
	);
	assert_eq!(refusals.count(Source::Crlite), 1);

	let opted_out = setup.client(
		&revocation,
		Sources {
			crlite: false,
			..Sources::all()
		},
		&refusals,
	);
	assert!(handshake(&opted_out, &server).is_ok());
}

#[test]
fn a_revocation_in_a_later_incremental_filter_is_picked_up() {
	let setup = Setup::new(true);
	let key = setup.tls.intermediate_spki_hash;
	let serial = setup.tls.leaf_serial.clone();
	let full = crlite_filter(&[(key, vec![0x7f])], &[(key, serial.clone())]);
	setup.publish_crlite(&[("full", full.clone(), false)], 10);

	let revocation = setup.revocation(None, POLL);
	revocation.enable(only(Source::Crlite));
	wait_for("CRLite", || {
		held_timestamp(&revocation, Source::Crlite) == Some(10)
	});

	let refusals = Arc::new(Refusals::default());
	let server = setup.server_config();
	let client = setup.client(&revocation, Sources::all(), &refusals);
	assert!(handshake(&client, &server).is_ok());

	let delta = crlite_filter(&[(key, serial)], &[(key, vec![0x7f])]);
	setup.publish_crlite(&[("full", full, false), ("delta", delta, true)], 20);
	wait_for("the delta", || {
		held_timestamp(&revocation, Source::Crlite) == Some(20)
	});
	assert_eq!(
		revoked_by(handshake(&client, &server)),
		Some(Source::Crlite)
	);

	// The full filter was already held, so only the delta was downloaded.
	let downloads: Vec<_> = setup
		.server
		.requests()
		.into_iter()
		.filter(|path| path.starts_with("/attachments/"))
		.collect();
	assert_eq!(downloads, vec!["/attachments/full", "/attachments/delta"]);
}

#[test]
fn a_missing_incremental_filter_is_retried() {
	let setup = Setup::new(true);
	let key = setup.tls.intermediate_spki_hash;
	let serial = setup.tls.leaf_serial.clone();
	let full = crlite_filter(&[(key, vec![0x7f])], &[(key, serial.clone())]);
	let delta = crlite_filter(&[(key, serial)], &[(key, vec![0x7f])]);
	setup.server.attach("full", full.clone());
	setup.server.publish(
		&setup.signing,
		"cert-revocations",
		vec![
			crlite_record("full", &full, "full", false),
			crlite_record("delta", &delta, "delta", true),
		],
		10,
	);

	let revocation = setup.revocation(None, POLL);
	revocation.enable(only(Source::Crlite));
	wait_for("the full filter", || {
		last_confirmed(&revocation, Source::Crlite).is_some()
	});

	let refusals = Arc::new(Refusals::default());
	let server = setup.server_config();
	let client = setup.client(&revocation, Sources::all(), &refusals);
	assert!(handshake(&client, &server).is_ok(), "the delta is not held");

	setup.server.attach("delta", delta);
	wait_for("the delta", || {
		revocation.inner.store.held(Source::Crlite).1
	});
	assert_eq!(
		revoked_by(handshake(&client, &server)),
		Some(Source::Crlite)
	);
}

#[test]
fn a_session_saved_before_an_update_is_not_resumed() {
	let setup = Setup::new(false);
	let unrelated = TlsPki::new(false);
	setup.publish_onecrl(vec![onecrl_issuer_serial(&unrelated.intermediate)], 10);
	let revocation = setup.revocation(None, POLL);
	revocation.enable(only(Source::OneCrl));
	wait_for("OneCRL", || {
		held_timestamp(&revocation, Source::OneCrl) == Some(10)
	});

	let refusals = Arc::new(Refusals::default());
	let server = setup.server_config();
	let client = setup.client(&revocation, Sources::all(), &refusals);
	assert_eq!(handshake(&client, &server), Ok(HandshakeKind::Full));
	assert_eq!(handshake(&client, &server), Ok(HandshakeKind::Resumed));

	setup.publish_onecrl(vec![onecrl_issuer_serial(&setup.tls.intermediate)], 20);
	wait_for("the update", || {
		held_timestamp(&revocation, Source::OneCrl) == Some(20)
	});
	assert_eq!(
		revoked_by(handshake(&client, &server)),
		Some(Source::OneCrl),
		"a resumption would have skipped the check"
	);
}

#[test]
fn a_ticket_arriving_after_an_update_is_checked_against_the_data_it_was_verified_under() {
	let setup = Setup::new(false);
	let unrelated = TlsPki::new(false);
	setup.publish_onecrl(vec![onecrl_issuer_serial(&unrelated.intermediate)], 10);
	let revocation = setup.revocation(None, POLL);
	revocation.enable(only(Source::OneCrl));
	wait_for("OneCRL", || {
		held_timestamp(&revocation, Source::OneCrl) == Some(10)
	});

	let refusals = Arc::new(Refusals::default());
	let client = setup.client(&revocation, Sources::all(), &refusals);
	let server = setup.server_config();

	// Run a handshake up to the client's Finished, which has the server queue its tickets, and
	// hold them back.
	let name = rustls::pki_types::ServerName::try_from("localhost").expect("a valid name");
	let mut client_conn = rustls::Connection::Client(
		rustls::ClientConnection::new(Arc::clone(&client), name).expect("a client"),
	);
	let mut server_conn = rustls::Connection::Server(
		rustls::ServerConnection::new(Arc::clone(&server)).expect("a server"),
	);
	transfer(&mut client_conn, &mut server_conn).expect("hello");
	transfer(&mut server_conn, &mut client_conn).expect("the server's flight, verified");
	transfer(&mut client_conn, &mut server_conn).expect("finished");
	assert!(server_conn.wants_write(), "the server has tickets to send");

	setup.publish_onecrl(vec![onecrl_issuer_serial(&unrelated.root)], 20);
	wait_for("the update", || {
		held_timestamp(&revocation, Source::OneCrl) == Some(20)
	});

	// The tickets arrive now, after the data they were checked against has been replaced.
	transfer(&mut server_conn, &mut client_conn).expect("the tickets");
	assert_eq!(
		handshake(&client, &server),
		Ok(HandshakeKind::Full),
		"a ticket from before the update must not resume"
	);
	assert_eq!(handshake(&client, &server), Ok(HandshakeKind::Resumed));
}

#[test]
fn the_platform_roots_are_indexed_by_the_time_enable_returns() {
	let setup = Setup::new(false);
	let revocation = setup.revocation(None, DEFAULT_POLL_INTERVAL);
	assert!(revocation.inner.store.roots.read().unwrap().is_none());

	revocation.enable(only(Source::OneCrl));
	assert!(revocation.inner.store.roots.read().unwrap().is_some());
}

#[test]
fn another_agent_does_not_cut_a_backoff_short() {
	let setup = Setup::new(false);
	setup.server.set_failing(true);
	let revocation = setup.revocation(None, DEFAULT_POLL_INTERVAL);
	revocation.enable(only(Source::OneCrl));
	wait_for("a failed check", || !setup.server.requests().is_empty());
	std::thread::sleep(Duration::from_millis(200));

	let before = setup.server.requests().len();
	revocation.enable(only(Source::OneCrl));
	std::thread::sleep(Duration::from_millis(500));
	assert_eq!(setup.server.requests().len(), before, "no retry yet");
}

#[test]
fn an_unchanged_collection_is_only_confirmed() {
	let setup = Setup::new(false);
	setup.publish_onecrl(vec![onecrl_issuer_serial(&setup.tls.intermediate)], 10);
	let revocation = setup.revocation(None, POLL);
	revocation.enable(only(Source::OneCrl));
	wait_for("OneCRL", || {
		last_confirmed(&revocation, Source::OneCrl).is_some()
	});

	let first = last_confirmed(&revocation, Source::OneCrl);
	let epoch = revocation.inner.store.epoch.load(Ordering::Acquire);
	wait_for("a second check", || {
		last_confirmed(&revocation, Source::OneCrl) > first
	});

	assert_eq!(revocation.inner.store.epoch.load(Ordering::Acquire), epoch);
	let changesets = setup
		.server
		.requests()
		.iter()
		.filter(|path| path.contains("/collections/onecrl/"))
		.count();
	assert_eq!(changesets, 1);
}

#[test]
fn a_bad_signature_keeps_the_data_held() {
	let setup = Setup::new(false);
	setup.publish_onecrl(vec![onecrl_issuer_serial(&setup.tls.intermediate)], 10);
	let revocation = setup.revocation(None, POLL);
	revocation.enable(only(Source::OneCrl));
	wait_for("OneCRL", || {
		last_confirmed(&revocation, Source::OneCrl).is_some()
	});

	let stripped = Vec::new();
	let signature = setup
		.signing
		.sign(&[onecrl_issuer_serial(&setup.tls.leaf)], 20);
	setup
		.server
		.publish_with_signature("onecrl", stripped, 20, signature);
	wait_for("a fetch of the bad collection", || {
		setup
			.server
			.requests()
			.iter()
			.any(|path| path.contains("onecrl/changeset?_expected=20"))
	});

	assert_eq!(held_timestamp(&revocation, Source::OneCrl), Some(10));
	let refusals = Arc::new(Refusals::default());
	let client = setup.client(&revocation, Sources::all(), &refusals);
	assert_eq!(
		revoked_by(handshake(&client, &setup.server_config())),
		Some(Source::OneCrl)
	);
}

#[test]
fn a_restart_enforces_the_cache_before_any_fetch() {
	let setup = Setup::new(true);
	let cache = tempfile::tempdir().expect("temp dir");
	setup.publish_onecrl(vec![onecrl_issuer_serial(&setup.tls.intermediate)], 10);
	let full = crlite_filter(
		&[(
			setup.tls.intermediate_spki_hash,
			setup.tls.leaf_serial.clone(),
		)],
		&[],
	);
	setup.publish_crlite(&[("full", full, false)], 10);

	let first = setup.revocation(Some(cache.path().to_owned()), DEFAULT_POLL_INTERVAL);
	first.enable(only(Source::OneCrl));
	first.enable(only(Source::Crlite));
	wait_for("both sources", || {
		last_confirmed(&first, Source::OneCrl).is_some()
			&& last_confirmed(&first, Source::Crlite).is_some()
	});
	let confirmed = last_confirmed(&first, Source::OneCrl);

	setup.server.set_failing(true);
	let requests = setup.server.requests().len();
	let second = setup.revocation(Some(cache.path().to_owned()), DEFAULT_POLL_INTERVAL);
	second.enable(only(Source::OneCrl));
	second.enable(only(Source::Crlite));

	// Enforced straight away, from the cache alone.
	let refusals = Arc::new(Refusals::default());
	let server = setup.server_config();
	let onecrl_only = setup.client(
		&second,
		Sources {
			crlite: false,
			..Sources::all()
		},
		&refusals,
	);
	assert_eq!(
		revoked_by(handshake(&onecrl_only, &server)),
		Some(Source::OneCrl)
	);
	let crlite_only = setup.client(
		&second,
		Sources {
			onecrl: false,
			..Sources::all()
		},
		&refusals,
	);
	assert_eq!(
		revoked_by(handshake(&crlite_only, &server)),
		Some(Source::Crlite)
	);

	// Confirmed at the time the first process confirmed it, to the second.
	let to_secs = |time: Option<SystemTime>| time.map(cache::secs);
	assert_eq!(
		to_secs(last_confirmed(&second, Source::OneCrl)),
		to_secs(confirmed)
	);

	// The cache was confirmed within the interval, so the second process asks for nothing.
	std::thread::sleep(Duration::from_millis(300));
	assert_eq!(setup.server.requests().len(), requests);
}

#[test]
fn a_tampered_cache_is_not_used() {
	let setup = Setup::new(false);
	let cache = tempfile::tempdir().expect("temp dir");
	setup.publish_onecrl(vec![onecrl_issuer_serial(&setup.tls.intermediate)], 10);

	let first = setup.revocation(Some(cache.path().to_owned()), DEFAULT_POLL_INTERVAL);
	first.enable(only(Source::OneCrl));
	wait_for("OneCRL", || {
		last_confirmed(&first, Source::OneCrl).is_some()
	});

	// Tamper with a copy: the first one's poller is free to rewrite its own cache when it confirms
	// the data, which would undo the tampering.
	let tampered = tempfile::tempdir().expect("temp dir");
	let mut cached: Value =
		serde_json::from_slice(&std::fs::read(cache.path().join("onecrl.json")).expect("cached"))
			.expect("cache parses");
	cached["signed"]["changeset"]["changes"] = Value::Array(Vec::new());
	std::fs::write(
		tampered.path().join("onecrl.json"),
		serde_json::to_vec(&cached).expect("serialises"),
	)
	.expect("written");

	setup.server.set_failing(true);
	let second = setup.revocation(Some(tampered.path().to_owned()), DEFAULT_POLL_INTERVAL);
	second.enable(only(Source::OneCrl));
	assert_eq!(last_confirmed(&second, Source::OneCrl), None);
}
