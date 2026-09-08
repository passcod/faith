//! Read an origin's `Alt-Svc` header into the store, then follow the upgrade decision it drives.
//!
//! Run with `cargo run -p web-faith-alt-svc --example upgrade`.

use std::time::Duration;

use reqwest::Url;
use web_faith_alt_svc::{AltSvcCache, AltSvcCacheConfig, parse_alt_svc_header};

fn report(cache: &AltSvcCache, url: &Url, stage: &str) {
	println!(
		"{stage}: route on {:?}, probe {:?}",
		cache.confirmed_port(url),
		cache.probe_candidate(url)
	);
}

fn main() {
	let cache = AltSvcCache::new(AltSvcCacheConfig::default());
	let origin = Url::parse("https://example.com/").expect("a valid URL");

	// An advertisement is evidence worth probing, not evidence worth routing on: it says the
	// alternative exists, not that it works.
	let advertisement =
		parse_alt_svc_header(r#"h3=":443"; ma=86400"#).expect("the header advertises h3");
	cache.record_alt_svc(&origin, &advertisement);
	report(&cache, &origin, "advertised");

	// A probe that reaches the origin over HTTP/3 is what promotes it to routable.
	let port = cache
		.probe_candidate(&origin)
		.expect("the advertisement is probe-worthy");
	cache.claim_probe(&origin);
	cache.confirm_h3(&origin, port);
	cache.finish_probe(&origin);
	report(&cache, &origin, "confirmed");

	// A confirmed origin that turns out sustainedly slower over HTTP/3 than the path it replaced
	// is demoted, and is not re-probed while the slow marker lives: once it lapses the preserved
	// advertisement re-enters through a probe, asking whether the path has improved.
	for _ in 0..16 {
		cache.record_path_time(&origin, http::Version::HTTP_11, Duration::from_millis(20));
		cache.record_path_time(&origin, http::Version::HTTP_3, Duration::from_millis(400));
	}
	report(&cache, &origin, "slow");

	// A network change drops what was learned about a network that no longer exists, keeping the
	// advertisement, which came from the origin rather than from the path.
	cache.network_changed();
	report(&cache, &origin, "after a network change");
}
