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

	advertised_ttl: Duration,
	confirmed_ttl: Duration,
}

impl std::fmt::Debug for AltSvcCache {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcCache")
			.field("advertised_count", &self.advertised.entry_count())
			.field("confirmed_count", &self.confirmed.entry_count())
			.field("failed_count", &self.failed.entry_count())
			.finish()
	}
}

impl AltSvcCache {
	pub fn new(
		advertised_ttl: Duration,
		confirmed_ttl: Duration,
		failed_ttl: Duration,
		capacity: u64,
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
			advertised_ttl,
			confirmed_ttl,
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

		let entry = AltSvcEntry {
			port,
			expires: Instant::now() + self.confirmed_ttl,
		};

		self.confirmed.insert(origin, entry);
	}

	pub fn record_h3_failure(&self, url: &reqwest::Url) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		self.advertised.invalidate(&origin);
		self.confirmed.invalidate(&origin);
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

#[derive(Clone)]
pub struct AltSvcMiddleware {
	cache: Arc<AltSvcCache>,
	enabled: bool,
}

impl std::fmt::Debug for AltSvcMiddleware {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcMiddleware")
			.field("enabled", &self.enabled)
			.field("cache", &self.cache)
			.finish()
	}
}

impl AltSvcMiddleware {
	pub fn new(cache: Arc<AltSvcCache>, enabled: bool) -> Self {
		Self { cache, enabled }
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
			*req.version_mut() = http::Version::HTTP_3;
		}

		let result = next.run(req, extensions).await;

		match &result {
			Ok(response) => {
				if trying_h3 && response.version() == http::Version::HTTP_3 {
					self.cache.confirm_h3(&url);
				}

				if let Some(alt_svc) = response.headers().get("alt-svc") {
					if let Ok(value) = alt_svc.to_str() {
						if let Some((port, max_age)) = parse_alt_svc_header(value) {
							self.cache.record_alt_svc(&url, port, max_age);
						}
					}
				}
			}
			Err(_) if trying_h3 => {
				self.cache.record_h3_failure(&url);
			}
			Err(_) => {}
		}

		result
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
		AltSvcCache::new(
			Duration::from_secs(86400),
			Duration::from_secs(86400),
			Duration::from_secs(300),
			10_000,
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
}
