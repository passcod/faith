//! Revocation checking through an agent, against a fake Remote Settings server.
//!
//! A test binary of its own, as the revocation data is process-wide: the server is installed in
//! place of Mozilla's before any agent starts checking.

// spec:REVOC spec:OBS

#![cfg(all(feature = "unstable-internals", feature = "onecrl", feature = "crlite"))]

use std::{
	io::{Read, Write},
	net::TcpListener,
	sync::Arc,
	time::{Duration, Instant},
};

use web_faith::{Agent, error::FaithErrorKind};
use web_faith_revocation::test_support::{
	FakeRemoteSettings, SigningPki, TlsPki, crlite_filter, crlite_record, onecrl_issuer_serial,
};

/// An HTTPS origin on loopback presenting `pki`'s leaf and intermediate, answering every request
/// with `200 ok`. Returns its port.
fn serve(pki: &TlsPki) -> u16 {
	let listener = TcpListener::bind("127.0.0.1:0").expect("binding loopback");
	let port = listener.local_addr().expect("bound").port();
	let config = Arc::new(pki.server_config());
	std::thread::spawn(move || {
		for mut stream in listener.incoming().flatten() {
			let config = Arc::clone(&config);
			std::thread::spawn(move || {
				let Ok(mut conn) = rustls::ServerConnection::new(config) else {
					return;
				};
				let mut tls = rustls::Stream::new(&mut conn, &mut stream);
				let mut request = Vec::new();
				let mut buf = [0u8; 4096];
				while !request.windows(4).any(|w| w == b"\r\n\r\n") {
					match tls.read(&mut buf) {
						Ok(0) | Err(_) => return,
						Ok(n) => request.extend_from_slice(&buf[..n]),
					}
				}
				let _ = tls.write_all(
					b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
				);
				let _ = tls.flush();
			});
		}
	});
	port
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
	let deadline = Instant::now() + Duration::from_secs(15);
	while !done() {
		assert!(Instant::now() < deadline, "timed out waiting for {what}");
		std::thread::sleep(Duration::from_millis(20));
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn agents_refuse_revoked_certificates() {
	let signing = SigningPki::new();
	let remote = FakeRemoteSettings::start(&signing);

	let listed_intermediate = TlsPki::new(true);
	let revoked_leaf = TlsPki::new(true);
	let clean = TlsPki::new(true);

	remote.publish(
		&signing,
		"onecrl",
		vec![onecrl_issuer_serial(&listed_intermediate.intermediate)],
		10,
	);
	let filter = crlite_filter(
		&[(
			revoked_leaf.intermediate_spki_hash,
			revoked_leaf.leaf_serial.clone(),
		)],
		&[(clean.intermediate_spki_hash, clean.leaf_serial.clone())],
	);
	remote.attach("full", filter.clone());
	remote.publish(
		&signing,
		"cert-revocations",
		vec![crlite_record("full", &filter, "full", false)],
		10,
	);

	let cache = tempfile::tempdir().expect("temp dir");
	assert!(web_faith::install_revocation(
		signing.config(&remote, Some(cache.path().to_owned()))
	));

	let roots = [
		listed_intermediate.root_pem.clone(),
		revoked_leaf.root_pem.clone(),
		clean.root_pem.clone(),
	];
	let agent = Agent::builder()
		.tls(|tls| tls.extra_roots(roots.clone()).build())
		.timeout(|timeout| timeout.total(Duration::from_secs(10)).build())
		.build()
		.expect("the options are valid");

	wait_for("both sources", || {
		let stats = agent.stats();
		stats.onecrl.is_some_and(|s| s.last_confirmed.is_some())
			&& stats.crlite.is_some_and(|s| s.last_confirmed.is_some())
	});

	let url = |pki: &TlsPki| format!("https://localhost:{}/", serve(pki));

	let ok = agent.fetch(url(&clean)).await.expect("a clean chain");
	assert_eq!(ok.status(), 200);

	let err = agent
		.fetch(url(&listed_intermediate))
		.await
		.expect_err("a listed intermediate");
	assert_eq!(err.kind(), FaithErrorKind::Network);
	assert!(
		err.to_string().contains("certificate revoked (OneCRL)"),
		"{err}"
	);

	let err = agent
		.fetch(url(&revoked_leaf))
		.await
		.expect_err("a revoked leaf");
	assert_eq!(err.kind(), FaithErrorKind::Network);
	assert!(
		err.to_string().contains("certificate revoked (CRLite)"),
		"{err}"
	);

	let stats = agent.stats();
	assert!(stats.onecrl.expect("on").refused >= 1);
	assert!(stats.crlite.expect("on").refused >= 1);

	// Opting out of a source lets what only it refuses through, and drops it from the stats.
	let opted_out = Agent::builder()
		.tls(|tls| {
			tls.extra_roots(roots.clone())
				.revocation(|revocation| revocation.onecrl(false).build())
				.build()
		})
		.build()
		.expect("the options are valid");
	let ok = opted_out
		.fetch(url(&listed_intermediate))
		.await
		.expect("OneCRL is off");
	assert_eq!(ok.status(), 200);
	assert!(
		opted_out.fetch(url(&revoked_leaf)).await.is_err(),
		"CRLite is still on"
	);
	let stats = opted_out.stats();
	assert_eq!(stats.onecrl, None);
	assert_eq!(stats.crlite.expect("on").refused, 1);

	// Each agent counts its own refusals; the data's age is the process's.
	assert_eq!(
		agent.stats().crlite.expect("on").last_confirmed,
		stats.crlite.expect("on").last_confirmed
	);

	// The data was written to the cache for the next process.
	assert!(cache.path().join("onecrl.json").exists());
	assert!(cache.path().join("cert-revocations.json").exists());
}
