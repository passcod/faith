//! Store the cookies a response set, then read back the header the next request should carry.
//!
//! Run with `cargo run -p web-faith-cookies --example jar`.

use http::HeaderValue;
use url::Url;
use web_faith_cookies::{CookieLimits, FaithJar};

fn main() {
	let jar = FaithJar::new(CookieLimits::default());
	let origin = Url::parse("https://example.com/login").expect("a valid URL");

	// As a response would set them: one plain, one host-locked, one that expires long past the cap.
	let set_cookie = [
		HeaderValue::from_static("session=abc123; Path=/; Secure; HttpOnly"),
		HeaderValue::from_static("__Host-csrf=xyz; Path=/; Secure"),
		HeaderValue::from_static("stale=1; Max-Age=999999999"),
	];
	jar.store_response_cookies(set_cookie.iter(), &origin);

	// A cookie can also be added by hand, the way a caller seeds a jar.
	jar.add_cookie_str("theme=dark; Path=/", &origin);

	match jar.request_cookie_header(&origin) {
		Some(header) => println!("Cookie: {}", header.to_str().expect("ASCII cookie values")),
		None => println!("the jar has nothing for {origin}"),
	}

	// A different origin sees none of them: the jar matches on host and path.
	let elsewhere = Url::parse("https://other.example/").expect("a valid URL");
	println!(
		"other origin: {:?}",
		jar.request_cookie_header(&elsewhere).is_none()
	);
}
