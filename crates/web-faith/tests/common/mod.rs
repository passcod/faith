//! Shared by the revocation tests, each of which is a binary of its own.

#![allow(dead_code)]

use std::{
	io::{Read, Write},
	net::TcpListener,
	sync::Arc,
	time::{Duration, Instant},
};

use web_faith_revocation::test_support::TlsPki;

/// An HTTPS origin on loopback presenting `pki`'s leaf and intermediate, answering every request
/// with `200 ok`. Returns its port.
pub fn serve(pki: &TlsPki) -> u16 {
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

pub fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
	let deadline = Instant::now() + Duration::from_secs(15);
	while !done() {
		assert!(Instant::now() < deadline, "timed out waiting for {what}");
		std::thread::sleep(Duration::from_millis(20));
	}
}
