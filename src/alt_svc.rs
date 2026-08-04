use std::{
	sync::Arc,
	time::{Duration, Instant},
};

use http::Extensions;
use moka::sync::Cache;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result};

#[derive(Debug, Clone)]
pub struct AltSvcEntry {
	pub port: u16,
	pub expires: Instant,
}

#[derive(Clone)]
pub struct AltSvcCache {
	advertised: Cache<String, AltSvcEntry>,
	confirmed: Cache<String, AltSvcEntry>,
	failed: Cache<String, ()>,
	/// Consecutive cancelled HTTP/3 attempts per origin. Entries expire on a TTL
	/// (the strike window), so a run has to be sustained to count.
	cancellations: Cache<String, u32>,

	advertised_ttl: Duration,
	confirmed_ttl: Duration,
	cancel_strikes: u32,
}

impl std::fmt::Debug for AltSvcCache {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcCache")
			.field("advertised_count", &self.advertised.entry_count())
			.field("confirmed_count", &self.confirmed.entry_count())
			.field("failed_count", &self.failed.entry_count())
			.field("cancellation_count", &self.cancellations.entry_count())
			.finish()
	}
}

impl AltSvcCache {
	pub fn new(
		advertised_ttl: Duration,
		confirmed_ttl: Duration,
		failed_ttl: Duration,
		capacity: u64,
		cancel_strikes: u32,
		strike_window: Duration,
	) -> Self {
		Self {
			advertised: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(advertised_ttl)
				.build(),
			confirmed: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(confirmed_ttl)
				.build(),
			failed: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(failed_ttl)
				.build(),
			cancellations: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(strike_window)
				.build(),
			advertised_ttl,
			confirmed_ttl,
			cancel_strikes,
		}
	}

	fn origin_key(url: &reqwest::Url) -> Option<String> {
		let host = url.host_str()?;
		let port = url.port_or_known_default()?;
		Some(format!("{}://{}:{}", url.scheme(), host, port))
	}

	pub fn record_alt_svc(&self, url: &reqwest::Url, h3_port: u16, max_age: Option<Duration>) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		if self.failed.contains_key(&origin) {
			return;
		}

		if self.confirmed.contains_key(&origin) {
			return;
		}

		let ttl = max_age.unwrap_or(self.advertised_ttl);
		let entry = AltSvcEntry {
			port: h3_port,
			expires: Instant::now() + ttl,
		};

		self.advertised.insert(origin, entry);
	}

	pub fn add_hint(&self, host: &str, port: u16) {
		let origin = format!("https://{}:{}", host, port);

		if self.failed.contains_key(&origin) {
			return;
		}

		let entry = AltSvcEntry {
			port,
			expires: Instant::now() + Duration::from_hours(10_000), // forever
		};

		self.advertised.insert(origin, entry);
	}

	pub fn should_use_h3(&self, url: &reqwest::Url) -> Option<u16> {
		let origin = Self::origin_key(url)?;

		if self.failed.contains_key(&origin) {
			return None;
		}

		if let Some(entry) = self.confirmed.get(&origin) {
			if entry.expires > Instant::now() {
				return Some(entry.port);
			}
		}

		if let Some(entry) = self.advertised.get(&origin) {
			if entry.expires > Instant::now() {
				return Some(entry.port);
			}
		}

		None
	}

	pub fn confirm_h3(&self, url: &reqwest::Url) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		let port = if let Some(entry) = self.advertised.get(&origin) {
			self.advertised.invalidate(&origin);
			entry.port
		} else if let Some(entry) = self.confirmed.get(&origin) {
			entry.port
		} else {
			url.port_or_known_default().unwrap_or(443)
		};

		// A working h3 response is proof of health; forget any strikes.
		self.cancellations.invalidate(&origin);

		let entry = AltSvcEntry {
			port,
			expires: Instant::now() + self.confirmed_ttl,
		};

		self.confirmed.insert(origin, entry);
	}

	/// Record an HTTP/3 attempt that was cancelled before producing an outcome.
	///
	/// This is weaker evidence than an error: the request never got to find out
	/// whether HTTP/3 worked, so a single cancellation says nothing about the
	/// origin. Only a sustained run of them demotes it, which keeps callers that
	/// routinely abort healthy requests from disabling HTTP/3.
	///
	/// The window is a TTL measured from the *previous* strike, because moka
	/// refreshes an entry's TTL on upsert. Strikes therefore have to arrive
	/// within a window of each other, not within a fixed bucket.
	pub fn record_h3_cancellation(&self, url: &reqwest::Url) {
		if self.cancel_strikes == 0 {
			return;
		}

		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		let strikes = self
			.cancellations
			.entry(origin)
			.and_upsert_with(|existing| existing.map_or(1, |entry| entry.into_value() + 1))
			.into_value();

		if strikes >= self.cancel_strikes {
			// Clears the strike count as a side effect.
			self.record_h3_failure(url);
		}
	}

	pub fn record_h3_failure(&self, url: &reqwest::Url) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		self.advertised.invalidate(&origin);
		self.confirmed.invalidate(&origin);
		// Already demoted; further counting is meaningless.
		self.cancellations.invalidate(&origin);
		self.failed.insert(origin, ());
	}
}

pub fn parse_alt_svc_header(value: &str) -> Option<(u16, Option<Duration>)> {
	if value == "clear" {
		return None;
	}

	for service in value.split(',') {
		let service = service.trim();
		if service.is_empty() {
			continue;
		}

		let mut protocol_id: Option<&str> = None;
		let mut port: Option<u16> = None;
		let mut max_age: Option<Duration> = None;

		for param in service.split(';') {
			let param = param.trim();
			if param.is_empty() {
				continue;
			}

			let Some((key, value)) = param.split_once('=') else {
				continue;
			};

			let key = key.trim();
			let value = value.trim().trim_matches('"');

			match key {
				"ma" => {
					if let Ok(secs) = value.parse::<u64>() {
						max_age = Some(Duration::from_secs(secs));
					}
				}
				_ if key.starts_with("h3") => {
					protocol_id = Some(key);
					if let Some((_, port_str)) = value.split_once(':') {
						if let Ok(p) = port_str.parse::<u16>() {
							port = Some(p);
						}
					}
				}
				_ => {}
			}
		}

		if protocol_id.is_some() && port.is_some() {
			return Some((port.unwrap(), max_age));
		}
	}

	None
}

/// Records a cancellation if the HTTP/3 attempt it guards is dropped before
/// producing an outcome.
///
/// [`AltSvcMiddleware`] can only learn that HTTP/3 is broken from the attempt's
/// return value, and a cancelled request never produces one: `faith_fetch`
/// races `send()` against the abort signal in a `select!`, which drops the
/// losing future. Without this guard nothing ever demotes the origin, so a
/// caller whose deadline is shorter than the network's own failure detection
/// re-attempts HTTP/3 over a dead path on every retry, indefinitely.
struct H3AttemptGuard {
	cache: Arc<AltSvcCache>,
	url: reqwest::Url,
	armed: bool,
}

impl H3AttemptGuard {
	fn new(cache: Arc<AltSvcCache>, url: reqwest::Url) -> Self {
		Self {
			cache,
			url,
			armed: true,
		}
	}

	/// The attempt produced an outcome, so it speaks for itself.
	fn disarm(&mut self) {
		self.armed = false;
	}
}

impl Drop for H3AttemptGuard {
	fn drop(&mut self) {
		// Must stay infallible: this can run while unwinding, where a panic
		// would abort the process. moka's sync cache does not panic on insert.
		if self.armed {
			self.cache.record_h3_cancellation(&self.url);
		}
	}
}

#[derive(Clone)]
pub struct AltSvcMiddleware {
	cache: Arc<AltSvcCache>,
	enabled: bool,
	/// Ceiling on how long an HTTP/3 attempt may take to produce response
	/// headers before it is treated as failed and retried over TCP.
	attempt_timeout: Option<Duration>,
}

impl std::fmt::Debug for AltSvcMiddleware {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcMiddleware")
			.field("enabled", &self.enabled)
			.field("attempt_timeout", &self.attempt_timeout)
			.field("cache", &self.cache)
			.finish()
	}
}

impl AltSvcMiddleware {
	pub fn new(cache: Arc<AltSvcCache>, enabled: bool, attempt_timeout: Option<Duration>) -> Self {
		Self {
			cache,
			enabled,
			attempt_timeout,
		}
	}

	#[allow(dead_code)]
	pub fn cache(&self) -> &Arc<AltSvcCache> {
		&self.cache
	}
}

#[async_trait::async_trait]
impl Middleware for AltSvcMiddleware {
	async fn handle(
		&self,
		mut req: Request,
		extensions: &mut Extensions,
		next: Next<'_>,
	) -> Result<Response> {
		if !self.enabled {
			return next.run(req, extensions).await;
		}

		let url = req.url().clone();
		let trying_h3 = self.cache.should_use_h3(&url).is_some();

		if trying_h3 {
			// Clone the request before attempting HTTP/3 so we can retry with TCP if it fails
			if let Some(req_clone) = req.try_clone() {
				*req.version_mut() = http::Version::HTTP_3;

				let mut guard = H3AttemptGuard::new(Arc::clone(&self.cache), url.clone());
				// `None` means the attempt ran out of time. Bound in its own
				// statement so the mutable borrow of `extensions` ends here,
				// leaving the fallback below free to use it.
				let outcome = match self.attempt_timeout {
					Some(limit) => tokio::time::timeout(limit, next.clone().run(req, extensions))
						.await
						.ok(),
					None => Some(next.clone().run(req, extensions).await),
				};
				// Reached on success, error and expiry alike; only a mid-flight
				// drop skips it and leaves the guard armed.
				guard.disarm();

				match outcome {
					Some(Ok(response)) => {
						if response.version() == http::Version::HTTP_3 {
							self.cache.confirm_h3(&url);
						}

						if let Some(alt_svc) = response.headers().get("alt-svc") {
							if let Ok(value) = alt_svc.to_str() {
								if let Some((port, max_age)) = parse_alt_svc_header(value) {
									self.cache.record_alt_svc(&url, port, max_age);
								}
							}
						}

						Ok(response)
					}
					// An expired deadline is as good as an error: HTTP/3 did not
					// deliver. Taking the fallback branch directly avoids having
					// to synthesise a reqwest_middleware::Error, which would mean
					// adding anyhow as a dependency.
					Some(Err(_)) | None => {
						self.cache.record_h3_failure(&url);

						// Use the cloned request (which still has default HTTP version)
						next.run(req_clone, extensions).await
					}
				}
			} else {
				// Can't clone request (streaming body), just proceed without HTTP/3
				next.run(req, extensions).await
			}
		} else {
			let result = next.run(req, extensions).await;

			// Check for Alt-Svc header in non-HTTP/3 responses
			if let Ok(ref response) = result {
				if let Some(alt_svc) = response.headers().get("alt-svc") {
					if let Ok(value) = alt_svc.to_str() {
						if let Some((port, max_age)) = parse_alt_svc_header(value) {
							self.cache.record_alt_svc(&url, port, max_age);
						}
					}
				}
			}

			result
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_alt_svc_simple() {
		let result = parse_alt_svc_header(r#"h3=":443"; ma=86400"#);
		assert_eq!(result, Some((443, Some(Duration::from_secs(86400)))));
	}

	#[test]
	fn test_parse_alt_svc_no_max_age() {
		let result = parse_alt_svc_header(r#"h3=":443""#);
		assert_eq!(result, Some((443, None)));
	}

	#[test]
	fn test_parse_alt_svc_different_port() {
		let result = parse_alt_svc_header(r#"h3=":8443"; ma=3600"#);
		assert_eq!(result, Some((8443, Some(Duration::from_secs(3600)))));
	}

	#[test]
	fn test_parse_alt_svc_multiple_protocols() {
		let result = parse_alt_svc_header(r#"h2=":443", h3=":443"; ma=86400"#);
		assert_eq!(result, Some((443, Some(Duration::from_secs(86400)))));
	}

	#[test]
	fn test_parse_alt_svc_h3_variant() {
		let result = parse_alt_svc_header(r#"h3-29=":443"; ma=86400"#);
		assert_eq!(result, Some((443, Some(Duration::from_secs(86400)))));
	}

	#[test]
	fn test_parse_alt_svc_clear() {
		let result = parse_alt_svc_header("clear");
		assert_eq!(result, None);
	}

	#[test]
	fn test_parse_alt_svc_no_h3() {
		let result = parse_alt_svc_header(r#"h2=":443"; ma=86400"#);
		assert_eq!(result, None);
	}

	fn test_cache() -> AltSvcCache {
		test_cache_with(3, Duration::from_secs(60))
	}

	fn test_cache_with(cancel_strikes: u32, strike_window: Duration) -> AltSvcCache {
		AltSvcCache::new(
			Duration::from_secs(86400),
			Duration::from_secs(86400),
			Duration::from_secs(300),
			10_000,
			cancel_strikes,
			strike_window,
		)
	}

	#[test]
	fn test_cache_flow() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		assert!(cache.should_use_h3(&url).is_none());

		cache.record_alt_svc(&url, 443, Some(Duration::from_secs(3600)));
		assert_eq!(cache.should_use_h3(&url), Some(443));

		cache.confirm_h3(&url);
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

		cache.record_alt_svc(&url, 443, None);
		assert!(cache.should_use_h3(&url).is_some());

		cache.record_h3_failure(&url);
		assert!(cache.should_use_h3(&url).is_none());

		cache.record_alt_svc(&url, 443, None);
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
	fn test_cancellation_below_threshold_keeps_h3() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

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
		cache.record_alt_svc(&url, 443, None);

		for _ in 0..3 {
			cache.record_h3_cancellation(&url);
		}

		assert!(
			cache.should_use_h3(&url).is_none(),
			"three strikes demotes the origin"
		);
		assert!(
			cache
				.failed
				.contains_key(&"https://example.com:443".to_string()),
			"demotion goes through the failed cache, so re-advertisement can't re-arm it"
		);
	}

	#[test]
	fn test_cancellation_reset_by_h3_success() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

		cache.record_h3_cancellation(&url);
		cache.record_h3_cancellation(&url);
		cache.confirm_h3(&url);
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
		let cache = test_cache_with(0, Duration::from_secs(60));
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

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
		let cache = test_cache_with(3, Duration::from_millis(50));
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

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
}
