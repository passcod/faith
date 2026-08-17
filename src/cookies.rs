//! The agent's cookie jar. (spec:COOK)
//!
//! `cookie_store` implements the classic RFC 6265 storage model, and reqwest's [`Jar`] wraps it in a
//! private field, so extending it means wrapping the store ourselves rather than the jar. What this
//! adds is the RFC 6265bis rules that mean something without a browsing context: the `__Host-` and
//! `__Secure-` name prefixes, a cap on how far ahead a cookie may expire, and caps on how many
//! cookies and how many bytes one server can accumulate. `SameSite` is left to the `cookie` crate to
//! parse and is never read, governing cross-site behaviour that only a first-party context has.
//!
//! [`Jar`]: reqwest::cookie::Jar

use std::{collections::HashMap, sync::RwLock, time::Duration};

use cookie::{Cookie as RawCookie, Expiration};
use cookie_store::{Cookie as StoredCookie, CookieStore as Store, StoreAction};
use reqwest::{Url, cookie::CookieStore, header::HeaderValue};
use time::OffsetDateTime;

/// A cookie may not persist beyond this by default. RFC 6265bis §5.5.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(400 * 24 * 60 * 60);
/// Default cap on one cookie's name plus value, in bytes. RFC 6265bis §5.6 sets this as a floor
/// servers may rely on; browsers implement it as the ceiling, and so do we.
pub const DEFAULT_MAX_SIZE: usize = 4096;
/// Default cap on cookies kept for one domain.
pub const DEFAULT_MAX_PER_HOST: usize = 180;
/// Default cap on cookies kept across the whole jar.
pub const DEFAULT_MAX_TOTAL: usize = 3000;

/// The caps a jar enforces, from the agent's `cookies` options.
#[derive(Debug, Clone)]
pub struct CookieLimits {
	/// How far ahead a cookie may expire; a longer expiry is reduced to this.
	pub max_age: Duration,
	/// Largest name-plus-value, in bytes, that will be stored.
	pub max_size: usize,
	/// Most cookies kept for any one domain.
	pub max_per_host: usize,
	/// Most cookies kept across the jar.
	pub max_total: usize,
}

impl Default for CookieLimits {
	fn default() -> Self {
		Self {
			max_age: DEFAULT_MAX_AGE,
			max_size: DEFAULT_MAX_SIZE,
			max_per_host: DEFAULT_MAX_PER_HOST,
			max_total: DEFAULT_MAX_TOTAL,
		}
	}
}

/// The triple `cookie_store` keys a cookie by, so an evicted key can be passed straight to
/// [`Store::remove`]: domain, path, name.
type CookieKey = (String, String, String);

fn key_of(cookie: &StoredCookie<'static>) -> CookieKey {
	(
		String::from(&cookie.domain),
		String::from(&cookie.path),
		cookie.name().to_owned(),
	)
}

/// Whether cookies received from this URL count as coming over a secure transport.
///
/// Browsers widen this to any "potentially trustworthy" origin, which takes in `http://localhost`;
/// the standard calls for `https`, so a `__Host-` cookie a browser would keep on a local dev
/// server is rejected here.
fn is_secure(url: &Url) -> bool {
	url.scheme() == "https"
}

/// The agent's cookie jar.
#[derive(Debug)]
pub struct FaithJar {
	limits: CookieLimits,
	inner: RwLock<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
	store: Store,
	/// When each stored cookie arrived, so the caps can evict the oldest. `cookie_store::Cookie`
	/// carries neither a creation nor a last-access time, so the order is tracked alongside it.
	order: HashMap<CookieKey, u64>,
	next_seq: u64,
}

impl FaithJar {
	pub fn new(limits: CookieLimits) -> Self {
		Self {
			limits,
			inner: RwLock::new(Inner::default()),
		}
	}

	/// Store one cookie received from `url`, as `agent.addCookie(url, cookie)` does.
	///
	/// A cookie that does not parse, or that any of the storage rules reject, is dropped silently,
	/// consistent with the jar's other no-op behaviours.
	pub fn add_cookie_str(&self, cookie: &str, url: &Url) {
		let Ok(raw) = RawCookie::parse(cookie.to_owned()) else {
			return;
		};

		self.store_one(raw, url);
	}

	/// Gate a cookie on the bis rules, then hand it to the classic storage model.
	///
	/// Gating on the way in rather than filtering on the way out is what makes the caps bound real
	/// memory, and what makes the rules apply the same to `addCookie` as to a `Set-Cookie` header.
	fn store_one(&self, raw: RawCookie<'static>, url: &Url) {
		let Some(raw) = self.sanitise(raw, url) else {
			return;
		};

		let mut inner = self.inner.write().unwrap();
		inner.insert(&raw, url, &self.limits);
	}

	/// Apply the rules that decide whether a cookie is storable at all, and reduce an over-long
	/// expiry to the cap. Returns `None` for a cookie that is rejected outright.
	fn sanitise(&self, mut raw: RawCookie<'static>, url: &Url) -> Option<RawCookie<'static>> {
		if raw.name().len() + raw.value().len() > self.limits.max_size {
			return None;
		}

		if !prefix_allows(&raw, url) {
			return None;
		}

		clamp_expiry(&mut raw, self.limits.max_age);
		Some(raw)
	}
}

/// Whether a `__Host-` or `__Secure-` name prefix permits this cookie to be stored.
///
/// The prefixes are matched case-sensitively, as RFC 6265bis §4.1.3 defines them: the rules are what
/// the prefix means, so a name that only differs in case carries no requirement.
fn prefix_allows(raw: &RawCookie<'_>, url: &Url) -> bool {
	let secure = raw.secure().unwrap_or(false) && is_secure(url);

	if raw.name().starts_with("__Host-") {
		// Bound to the exact host that set it, at the root path.
		return secure && raw.domain().is_none() && raw.path() == Some("/");
	}

	if raw.name().starts_with("__Secure-") {
		return secure;
	}

	true
}

/// Reduce an expiry further ahead than `max_age` to `max_age` from now.
///
/// `Max-Age` is checked first and returned on, because that is the precedence the storage model
/// reads them in: a cookie carrying both takes its expiry from `Max-Age`, so clamping `Expires`
/// there would cap an expiry nothing consults. A session cookie has neither and stays one.
fn clamp_expiry(raw: &mut RawCookie<'static>, max_age: Duration) {
	let cap = time::Duration::try_from(max_age).unwrap_or(time::Duration::MAX);

	if let Some(max_age) = raw.max_age() {
		if max_age > cap {
			raw.set_max_age(cap);
		}

		return;
	}

	if let Some(Expiration::DateTime(expires)) = raw.expires() {
		let limit = OffsetDateTime::now_utc().saturating_add(cap);
		if expires > limit {
			raw.set_expires(limit);
		}
	}
}

impl Inner {
	fn insert(&mut self, raw: &RawCookie<'static>, url: &Url, limits: &CookieLimits) {
		// The key is derived the same way the store derives it, so an eviction can name the cookie
		// back to the store. A cookie the classic model rejects fails here too, and is dropped.
		let Ok(parsed) = StoredCookie::try_from_raw_cookie(raw, url) else {
			return;
		};
		let key = key_of(&parsed);

		match self.store.insert_raw(raw, url) {
			// A cookie that replaces one already stored keeps the original's place in the order,
			// so a session refreshed on every request does not outlive older cookies by being
			// rewritten.
			Ok(StoreAction::Inserted | StoreAction::UpdatedExisting) => {
				if !self.order.contains_key(&key) {
					self.order.insert(key.clone(), self.next_seq);
					self.next_seq += 1;
				}
			}
			// The cookie was not stored: either it expired an existing cookie in place, which the
			// expiry purge collects, or the storage model rejected it.
			Ok(StoreAction::ExpiredExisting) | Err(_) => return,
		}

		self.enforce(&key.0, limits);
	}

	/// Bring the jar back within its caps, per domain and then overall.
	///
	/// Trimming after the insert rather than before keeps the incoming cookie the newest, so
	/// oldest-first eviction never picks it while an older cookie remains.
	fn enforce(&mut self, domain: &str, limits: &CookieLimits) {
		if self.count(Some(domain)) > limits.max_per_host {
			self.purge_expired();
			self.evict_oldest(Some(domain), limits.max_per_host);
		}

		if self.count(None) > limits.max_total {
			self.purge_expired();
			self.evict_oldest(None, limits.max_total);
		}
	}

	fn count(&self, domain: Option<&str>) -> usize {
		match domain {
			None => self.order.len(),
			Some(domain) => self.order.keys().filter(|(d, ..)| d == domain).count(),
		}
	}

	/// Drop cookies that have expired, so a cap evicts live cookies only once dead ones are gone.
	fn purge_expired(&mut self) {
		let expired: Vec<CookieKey> = self
			.store
			.iter_any()
			.filter(|cookie| cookie.is_expired())
			.map(key_of)
			.collect();

		for key in expired {
			self.remove(&key);
		}
	}

	/// Evict oldest-first until at most `cap` cookies remain in scope.
	fn evict_oldest(&mut self, domain: Option<&str>, cap: usize) {
		let mut scoped: Vec<(u64, CookieKey)> = self
			.order
			.iter()
			.filter(|((d, ..), _)| domain.is_none_or(|domain| d == domain))
			.map(|(key, seq)| (*seq, key.clone()))
			.collect();

		let excess = scoped.len().saturating_sub(cap);
		if excess == 0 {
			return;
		}

		scoped.sort_unstable();
		for (_, key) in scoped.into_iter().take(excess) {
			self.remove(&key);
		}
	}

	/// Remove a cookie from the store and from the order alongside it, so the two never drift.
	fn remove(&mut self, key: &CookieKey) {
		let (domain, path, name) = key;
		self.store.remove(domain, path, name);
		self.order.remove(key);
	}
}

impl CookieStore for FaithJar {
	fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
		for header in cookie_headers {
			let Ok(header) = std::str::from_utf8(header.as_bytes()) else {
				continue;
			};

			let Ok(raw) = RawCookie::parse(header.to_owned()) else {
				continue;
			};

			self.store_one(raw, url);
		}
	}

	fn cookies(&self, url: &Url) -> Option<HeaderValue> {
		let inner = self.inner.read().unwrap();
		let cookies = inner
			.store
			.get_request_values(url)
			.map(|(name, value)| format!("{name}={value}"))
			.collect::<Vec<_>>()
			.join("; ");

		if cookies.is_empty() {
			return None;
		}

		HeaderValue::from_str(&cookies).ok()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn url(url: &str) -> Url {
		Url::parse(url).unwrap()
	}

	fn jar() -> FaithJar {
		FaithJar::new(CookieLimits::default())
	}

	fn jar_with(limits: CookieLimits) -> FaithJar {
		FaithJar::new(limits)
	}

	/// What the jar would send to `url`, as a `Cookie` header value.
	fn sent(jar: &FaithJar, url: &str) -> Option<String> {
		jar.cookies(&self::url(url))
			.map(|value| value.to_str().unwrap().to_owned())
	}

	fn stored_count(jar: &FaithJar) -> usize {
		jar.inner.read().unwrap().store.iter_unexpired().count()
	}

	// Name prefixes

	#[test]
	fn host_prefix_accepted_when_fully_qualified() {
		let jar = jar();
		jar.add_cookie_str("__Host-a=1; Secure; Path=/", &url("https://example.com/"));
		assert_eq!(
			sent(&jar, "https://example.com/"),
			Some("__Host-a=1".into())
		);
	}

	#[test]
	fn host_prefix_rejected_without_secure_attribute() {
		let jar = jar();
		jar.add_cookie_str("__Host-a=1; Path=/", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}

	#[test]
	fn host_prefix_rejected_over_insecure_transport() {
		let jar = jar();
		jar.add_cookie_str("__Host-a=1; Secure; Path=/", &url("http://example.com/"));
		assert_eq!(sent(&jar, "http://example.com/"), None);
	}

	#[test]
	fn host_prefix_rejected_with_domain_attribute() {
		let jar = jar();
		jar.add_cookie_str(
			"__Host-a=1; Secure; Path=/; Domain=example.com",
			&url("https://example.com/"),
		);
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}

	#[test]
	fn host_prefix_rejected_with_non_root_path() {
		let jar = jar();
		jar.add_cookie_str(
			"__Host-a=1; Secure; Path=/app",
			&url("https://example.com/app"),
		);
		assert_eq!(sent(&jar, "https://example.com/app"), None);
	}

	#[test]
	fn host_prefix_rejected_without_path_attribute() {
		let jar = jar();
		jar.add_cookie_str("__Host-a=1; Secure", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}

	#[test]
	fn secure_prefix_accepted_with_secure_attribute() {
		let jar = jar();
		jar.add_cookie_str("__Secure-a=1; Secure", &url("https://example.com/"));
		assert_eq!(
			sent(&jar, "https://example.com/"),
			Some("__Secure-a=1".into())
		);
	}

	#[test]
	fn secure_prefix_allows_domain_and_path_unlike_host() {
		let jar = jar();
		jar.add_cookie_str(
			"__Secure-a=1; Secure; Path=/app; Domain=example.com",
			&url("https://example.com/app"),
		);
		assert_eq!(
			sent(&jar, "https://sub.example.com/app"),
			Some("__Secure-a=1".into())
		);
	}

	#[test]
	fn secure_prefix_rejected_without_secure_attribute() {
		let jar = jar();
		jar.add_cookie_str("__Secure-a=1", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}

	#[test]
	fn secure_prefix_rejected_over_insecure_transport() {
		let jar = jar();
		jar.add_cookie_str("__Secure-a=1; Secure", &url("http://example.com/"));
		assert_eq!(sent(&jar, "http://example.com/"), None);
	}

	#[test]
	fn prefixes_are_matched_case_sensitively() {
		let jar = jar();
		// Differing in case, these carry no prefix requirement at all, so they store as ordinary
		// cookies over an insecure transport.
		jar.add_cookie_str("__host-a=1", &url("http://example.com/"));
		jar.add_cookie_str("__SECURE-b=2", &url("http://example.com/"));

		let sent = sent(&jar, "http://example.com/").unwrap();
		assert!(sent.contains("__host-a=1"), "{sent}");
		assert!(sent.contains("__SECURE-b=2"), "{sent}");
	}

	#[test]
	fn unprefixed_cookies_are_unaffected() {
		let jar = jar();
		jar.add_cookie_str("a=1", &url("http://example.com/"));
		assert_eq!(sent(&jar, "http://example.com/"), Some("a=1".into()));
	}

	// Expiry cap

	#[test]
	fn over_long_max_age_is_reduced_to_the_cap() {
		let jar = jar();
		let over = DEFAULT_MAX_AGE.as_secs() * 2;
		jar.add_cookie_str(
			&format!("a=1; Max-Age={over}"),
			&url("https://example.com/"),
		);

		let inner = jar.inner.read().unwrap();
		let cookie = inner.store.get("example.com", "/", "a").unwrap();
		let cap = OffsetDateTime::now_utc() + time::Duration::try_from(DEFAULT_MAX_AGE).unwrap();
		assert!(
			cookie.expires_by(&(cap + time::Duration::minutes(1))),
			"expiry should have been reduced to the cap"
		);
	}

	#[test]
	fn over_long_expires_is_reduced_to_the_cap() {
		let jar = jar();
		jar.add_cookie_str(
			"a=1; Expires=Fri, 31 Dec 9999 23:59:59 GMT",
			&url("https://example.com/"),
		);

		let inner = jar.inner.read().unwrap();
		let cookie = inner.store.get("example.com", "/", "a").unwrap();
		let cap = OffsetDateTime::now_utc() + time::Duration::try_from(DEFAULT_MAX_AGE).unwrap();
		assert!(
			cookie.expires_by(&(cap + time::Duration::minutes(1))),
			"expiry should have been reduced to the cap"
		);
	}

	#[test]
	fn shorter_expiry_is_left_alone() {
		let jar = jar();
		jar.add_cookie_str("a=1; Max-Age=60", &url("https://example.com/"));

		let inner = jar.inner.read().unwrap();
		let cookie = inner.store.get("example.com", "/", "a").unwrap();
		assert!(cookie.expires_by(&(OffsetDateTime::now_utc() + time::Duration::minutes(2))));
		assert!(!cookie.expires_by(&(OffsetDateTime::now_utc() + time::Duration::seconds(30))));
	}

	#[test]
	fn session_cookie_stays_a_session_cookie() {
		let jar = jar();
		jar.add_cookie_str("a=1", &url("https://example.com/"));

		let inner = jar.inner.read().unwrap();
		let cookie = inner.store.get("example.com", "/", "a").unwrap();
		assert!(!cookie.is_persistent(), "should not have gained an expiry");
	}

	#[test]
	fn max_age_takes_precedence_over_expires() {
		let jar = jar();
		// Max-Age is within the cap, so the far-future Expires is never consulted and the cookie
		// keeps its one-minute life.
		jar.add_cookie_str(
			"a=1; Max-Age=60; Expires=Fri, 31 Dec 9999 23:59:59 GMT",
			&url("https://example.com/"),
		);

		let inner = jar.inner.read().unwrap();
		let cookie = inner.store.get("example.com", "/", "a").unwrap();
		assert!(cookie.expires_by(&(OffsetDateTime::now_utc() + time::Duration::minutes(2))));
	}

	#[test]
	fn expiring_a_cookie_still_works() {
		// A server removes a cookie by resending it already expired; the cap must not get in the way.
		let jar = jar();
		jar.add_cookie_str("a=1", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), Some("a=1".into()));

		jar.add_cookie_str("a=1; Max-Age=0", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}

	#[test]
	fn max_age_cap_is_configurable() {
		let jar = jar_with(CookieLimits {
			max_age: Duration::from_secs(60),
			..Default::default()
		});
		jar.add_cookie_str("a=1; Max-Age=86400", &url("https://example.com/"));

		let inner = jar.inner.read().unwrap();
		let cookie = inner.store.get("example.com", "/", "a").unwrap();
		assert!(cookie.expires_by(&(OffsetDateTime::now_utc() + time::Duration::minutes(2))));
	}

	// Size cap

	#[test]
	fn oversized_cookie_is_rejected() {
		let jar = jar();
		let value = "x".repeat(DEFAULT_MAX_SIZE);
		jar.add_cookie_str(&format!("a={value}"), &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}

	#[test]
	fn cookie_at_exactly_the_size_cap_is_stored() {
		let jar = jar();
		let value = "x".repeat(DEFAULT_MAX_SIZE - 1);
		jar.add_cookie_str(&format!("a={value}"), &url("https://example.com/"));
		assert_eq!(stored_count(&jar), 1);
	}

	#[test]
	fn size_cap_counts_name_and_value_together() {
		let jar = jar_with(CookieLimits {
			max_size: 10,
			..Default::default()
		});
		// The value alone is under the cap; with the name it is over.
		jar.add_cookie_str("name=1234567", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);

		jar.add_cookie_str("name=123456", &url("https://example.com/"));
		assert_eq!(
			sent(&jar, "https://example.com/"),
			Some("name=123456".into())
		);
	}

	#[test]
	fn size_cap_does_not_count_attributes() {
		let jar = jar_with(CookieLimits {
			max_size: 10,
			..Default::default()
		});
		jar.add_cookie_str(
			"name=12345; Path=/; Secure; HttpOnly",
			&url("https://example.com/"),
		);
		assert_eq!(stored_count(&jar), 1);
	}

	// Count caps

	#[test]
	fn per_host_cap_evicts_the_oldest() {
		let jar = jar_with(CookieLimits {
			max_per_host: 3,
			..Default::default()
		});
		for n in 0..5 {
			jar.add_cookie_str(&format!("c{n}=1"), &url("https://example.com/"));
		}

		let sent = sent(&jar, "https://example.com/").unwrap();
		assert_eq!(stored_count(&jar), 3, "{sent}");
		assert!(!sent.contains("c0="), "oldest should have gone: {sent}");
		assert!(
			!sent.contains("c1="),
			"next-oldest should have gone: {sent}"
		);
		assert!(sent.contains("c4=1"), "newest should have stayed: {sent}");
	}

	#[test]
	fn per_host_cap_is_per_domain() {
		let jar = jar_with(CookieLimits {
			max_per_host: 2,
			..Default::default()
		});
		for n in 0..3 {
			jar.add_cookie_str(&format!("c{n}=1"), &url("https://one.example/"));
			jar.add_cookie_str(&format!("c{n}=1"), &url("https://two.example/"));
		}

		assert_eq!(stored_count(&jar), 4, "each domain keeps its own allowance");
	}

	#[test]
	fn refreshing_a_cookie_keeps_its_place_in_the_order() {
		let jar = jar_with(CookieLimits {
			max_per_host: 2,
			..Default::default()
		});
		jar.add_cookie_str("a=1", &url("https://example.com/"));
		jar.add_cookie_str("b=1", &url("https://example.com/"));
		// Rewriting `a` must not make it younger than `b`, else a session cookie refreshed on every
		// response would evict everything else in turn.
		jar.add_cookie_str("a=2", &url("https://example.com/"));
		jar.add_cookie_str("c=1", &url("https://example.com/"));

		let sent = sent(&jar, "https://example.com/").unwrap();
		assert!(
			!sent.contains("a="),
			"a was oldest and should have gone: {sent}"
		);
		assert!(sent.contains("b=1"), "{sent}");
		assert!(sent.contains("c=1"), "{sent}");
	}

	#[test]
	fn expired_cookies_are_purged_before_live_ones_are_evicted() {
		let jar = jar_with(CookieLimits {
			max_per_host: 3,
			..Default::default()
		});
		// Two that die a second from now, then two that outlive them.
		jar.add_cookie_str("dead1=1; Max-Age=1", &url("https://example.com/"));
		jar.add_cookie_str("dead2=1; Max-Age=1", &url("https://example.com/"));
		jar.add_cookie_str("live1=1", &url("https://example.com/"));

		std::thread::sleep(Duration::from_millis(1100));

		// Storing this exceeds the cap; the two dead cookies go and `live1` survives.
		jar.add_cookie_str("live2=1", &url("https://example.com/"));

		let sent = sent(&jar, "https://example.com/").unwrap();
		assert!(sent.contains("live1=1"), "{sent}");
		assert!(sent.contains("live2=1"), "{sent}");
		assert_eq!(stored_count(&jar), 2, "{sent}");
	}

	#[test]
	fn whole_jar_cap_bounds_cookies_spread_across_domains() {
		let jar = jar_with(CookieLimits {
			max_per_host: 100,
			max_total: 5,
			..Default::default()
		});
		for n in 0..10 {
			jar.add_cookie_str("a=1", &url(&format!("https://host{n}.example/")));
		}

		assert_eq!(stored_count(&jar), 5);
		assert_eq!(
			sent(&jar, "https://host0.example/"),
			None,
			"oldest domain evicted"
		);
		assert_eq!(sent(&jar, "https://host9.example/"), Some("a=1".into()));
	}

	#[test]
	fn domain_scoped_cookies_are_evictable() {
		// A cookie carrying `Domain` is keyed differently from a host-only one, and eviction names
		// a cookie back to the store by that key: if the two disagreed, `remove` would quietly miss
		// and the jar would drift past its cap.
		let jar = jar_with(CookieLimits {
			max_per_host: 2,
			..Default::default()
		});
		for n in 0..4 {
			jar.add_cookie_str(
				&format!("c{n}=1; Domain=example.com"),
				&url("https://example.com/"),
			);
		}

		let sent = sent(&jar, "https://www.example.com/").unwrap();
		assert_eq!(stored_count(&jar), 2, "{sent}");
		assert!(!sent.contains("c0="), "oldest should have gone: {sent}");
		assert!(sent.contains("c3=1"), "newest should have stayed: {sent}");
	}

	#[test]
	fn a_cap_of_zero_stores_nothing() {
		let jar = jar_with(CookieLimits {
			max_per_host: 0,
			..Default::default()
		});
		jar.add_cookie_str("a=1", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}

	// Storage model

	#[test]
	fn cookies_are_scoped_to_their_domain() {
		let jar = jar();
		jar.add_cookie_str("a=1", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://elsewhere.example/"), None);
	}

	#[test]
	fn set_cookies_applies_the_same_rules_as_add_cookie() {
		let jar = jar();
		let headers = [
			HeaderValue::from_static("__Host-good=1; Secure; Path=/"),
			HeaderValue::from_static("__Host-bad=1; Path=/"),
		];

		jar.set_cookies(&mut headers.iter(), &url("https://example.com/"));

		let sent = sent(&jar, "https://example.com/").unwrap();
		assert!(sent.contains("__Host-good=1"), "{sent}");
		assert!(!sent.contains("__Host-bad"), "{sent}");
	}

	#[test]
	fn unparseable_cookies_are_dropped_silently() {
		let jar = jar();
		jar.add_cookie_str("", &url("https://example.com/"));
		assert_eq!(sent(&jar, "https://example.com/"), None);
	}
}
