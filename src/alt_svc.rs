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

/// An HTTP/3 alternative parsed out of an `Alt-Svc` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AltSvcAdvertisement {
	/// Host the alternative is on. Empty when the header omitted it, which per
	/// RFC 7838 means the same host as the origin.
	pub host: String,
	pub port: u16,
	pub max_age: Option<Duration>,
}

/// A per-origin exponentially-weighted moving average of time-to-response-headers.
///
/// Two `f64`s per origin and no sample storage: the average decays stale history
/// by construction, and the count gates decisions until there is enough evidence
/// to mean anything.
#[derive(Debug, Clone, Copy)]
pub struct PathTime {
	/// EWMA of time-to-response-headers, in milliseconds.
	pub avg_ms: f64,
	pub count: u32,
}

/// Weight of the newest sample in the moving average.
const EWMA_ALPHA: f64 = 0.2;
/// Samples required on *each* side before a slow comparison may act.
const EWMA_MIN_SAMPLES: u32 = 8;
/// Absolute gap the QUIC average must exceed the TCP one by, on top of the
/// factor, so LAN-fast origins don't flap on sub-millisecond noise.
const SLOW_FLOOR_MS: f64 = 10.0;

pub struct AltSvcCacheConfig {
	pub advertised_ttl: Duration,
	pub confirmed_ttl: Duration,
	pub failed_ttl: Duration,
	pub capacity: u64,
	pub cancel_strikes: u32,
	pub strike_window: Duration,
	pub follow_advertised_port: bool,
	/// Lifetime of a probe's single-flight claim. Doubles as crash recovery: a
	/// probe task that dies without reporting frees its origin when this lapses.
	pub probe_ttl: Duration,
	/// The QUIC path is demoted when its average is worse than TCP's by this
	/// factor (and by [`SLOW_FLOOR_MS`] absolutely). `0.0` disables path-time
	/// demotion entirely.
	pub slow_factor: f64,
	/// How long a path-time demotion holds before the origin may be re-probed.
	pub slow_ttl: Duration,
}

#[derive(Clone)]
pub struct AltSvcCache {
	advertised: Cache<String, AltSvcEntry>,
	confirmed: Cache<String, AltSvcEntry>,
	failed: Cache<String, ()>,
	/// Consecutive cancelled HTTP/3 attempts per origin. Entries expire on a TTL
	/// (the strike window), so a run has to be sustained to count.
	cancellations: Cache<String, u32>,
	/// Single-flight claims for in-flight background probes.
	probing: Cache<String, ()>,
	/// Origins demoted for being slower over QUIC than over TCP. Distinct from
	/// `failed`: the path *works*, so re-advertisements must not be discarded,
	/// and expiry re-enters through a probe rather than treating h3 as broken.
	slow: Cache<String, ()>,
	/// Time-to-headers over TCP (h1 and h2 together), per origin.
	tcp_times: Cache<String, PathTime>,
	/// Time-to-headers over QUIC (h3), per origin.
	quic_times: Cache<String, PathTime>,

	advertised_ttl: Duration,
	confirmed_ttl: Duration,
	cancel_strikes: u32,
	follow_advertised_port: bool,
	slow_factor: f64,
}

impl std::fmt::Debug for AltSvcCache {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcCache")
			.field("advertised_count", &self.advertised.entry_count())
			.field("confirmed_count", &self.confirmed.entry_count())
			.field("failed_count", &self.failed.entry_count())
			.field("cancellation_count", &self.cancellations.entry_count())
			.field("probing_count", &self.probing.entry_count())
			.field("slow_count", &self.slow.entry_count())
			.finish()
	}
}

impl AltSvcCache {
	pub fn new(config: AltSvcCacheConfig) -> Self {
		let AltSvcCacheConfig {
			advertised_ttl,
			confirmed_ttl,
			failed_ttl,
			capacity,
			cancel_strikes,
			strike_window,
			follow_advertised_port,
			probe_ttl,
			slow_factor,
			slow_ttl,
		} = config;

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
			probing: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(probe_ttl)
				.build(),
			slow: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(slow_ttl)
				.build(),
			tcp_times: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(confirmed_ttl)
				.build(),
			quic_times: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(confirmed_ttl)
				.build(),
			advertised_ttl,
			confirmed_ttl,
			cancel_strikes,
			follow_advertised_port,
			slow_factor,
		}
	}

	fn origin_key(url: &reqwest::Url) -> Option<String> {
		let host = url.host_str()?;
		let port = url.port_or_known_default()?;
		Some(format!("{}://{}:{}", url.scheme(), host, port))
	}

	pub fn record_alt_svc(&self, url: &reqwest::Url, advertisement: &AltSvcAdvertisement) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		// An alternative on a *different host* can never be honoured: reqwest derives
		// the HTTP/3 connect target from the request's authority, and rewriting the
		// host would also change which certificate is accepted. Unlike a differing
		// port — which `follow_advertised_port` can act on — there is nothing to
		// gate behind an option, so don't record it at all. RFC 7838 uses an empty
		// host to mean "the same host as the origin".
		//
		// Compared case-insensitively because host names are, and a server naming its
		// own host in a different case is still naming its own host.
		if !advertisement.host.is_empty()
			&& !url
				.host_str()
				.is_some_and(|origin_host| origin_host.eq_ignore_ascii_case(&advertisement.host))
		{
			return;
		}

		if self.failed.contains_key(&origin) {
			return;
		}

		if self.confirmed.contains_key(&origin) {
			return;
		}

		let ttl = advertisement.max_age.unwrap_or(self.advertised_ttl);
		let entry = AltSvcEntry {
			port: advertisement.port,
			expires: Instant::now() + ttl,
		};

		self.advertised.insert(origin, entry);
	}

	/// Hints seed `confirmed` directly, not `advertised`: a hint is the *user's*
	/// assertion, and routing it through a probe would both second-guess an
	/// explicit instruction and break h3-only origins (no TCP listener), which
	/// only work if the very first request speaks HTTP/3. Distrust is reserved
	/// for what servers advertise. Failure demotes a hinted origin exactly as it
	/// does a confirmed one.
	pub fn add_hint(&self, host: &str, port: u16) {
		let origin = format!("https://{}:{}", host, port);

		if self.failed.contains_key(&origin) {
			return;
		}

		let entry = AltSvcEntry {
			port,
			expires: Instant::now() + Duration::from_hours(10_000), // forever
		};

		self.confirmed.insert(origin, entry);
	}

	/// Whether an entry advertising `entry_port` can be acted on for this URL.
	///
	/// An Alt-Svc advertisement names a network endpoint for the origin; it is not
	/// a claim that the origin's *own* port speaks HTTP/3. So when the advertised
	/// port differs, upgrading the request on the origin port is an inference the
	/// advertisement does not support.
	///
	/// Honouring the advertised port properly means connecting to one port while
	/// still sending the origin's authority, which reqwest cannot express: it
	/// derives the HTTP/3 connect target from the request URI's authority (see
	/// <https://github.com/seanmonstar/reqwest/issues/1138>). `follow_advertised_port`
	/// opts into doing it anyway by rewriting the request's port, which is not
	/// standards-compliant — the request then carries the alternative's authority
	/// rather than the origin's.
	fn port_actionable(&self, url: &reqwest::Url, entry_port: u16) -> bool {
		self.follow_advertised_port || Some(entry_port) == url.port_or_known_default()
	}

	/// The port HTTP/3 is *proven* on, or `None` to leave the request on TCP.
	///
	/// This is the only lookup foreground routing consults when probing is on:
	/// an advertisement is evidence worth probing, not worth routing on.
	///
	/// A returned port that differs from the URL's own means the caller opted into
	/// `follow_advertised_port` and the request must be rewritten to target it.
	pub fn confirmed_port(&self, url: &reqwest::Url) -> Option<u16> {
		let origin = Self::origin_key(url)?;

		if self.failed.contains_key(&origin) || self.slow.contains_key(&origin) {
			return None;
		}

		let entry = self.confirmed.get(&origin)?;
		if entry.expires > Instant::now() && self.port_actionable(url, entry.port) {
			Some(entry.port)
		} else {
			None
		}
	}

	/// The advertised port a background probe should verify, or `None` when
	/// there is nothing (or no need) to probe: no actionable advertisement,
	/// already confirmed, recently failed, or demoted for being slow.
	pub fn probe_candidate(&self, url: &reqwest::Url) -> Option<u16> {
		let origin = Self::origin_key(url)?;

		if self.failed.contains_key(&origin)
			|| self.slow.contains_key(&origin)
			|| self.confirmed.contains_key(&origin)
		{
			return None;
		}

		let entry = self.advertised.get(&origin)?;
		if entry.expires > Instant::now() && self.port_actionable(url, entry.port) {
			Some(entry.port)
		} else {
			None
		}
	}

	/// Claim the origin for a probe. Returns `false` when a probe is already in
	/// flight; the claim expires on its own (see [`AltSvcCacheConfig::probe_ttl`])
	/// if the prober never reports back.
	pub fn claim_probe(&self, url: &reqwest::Url) -> bool {
		let Some(origin) = Self::origin_key(url) else {
			return false;
		};
		self.probing.entry(origin).or_insert(()).is_fresh()
	}

	/// Release the origin's probe claim, so a later advertisement can re-probe
	/// without waiting out the claim's TTL.
	pub fn finish_probe(&self, url: &reqwest::Url) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};
		self.probing.invalidate(&origin);
	}

	/// The port to attempt HTTP/3 on, or `None` to leave the request on TCP.
	///
	/// Legacy (probe-less) routing: advertisements are acted on inline, so this
	/// consults `advertised` as well as `confirmed`. Only used when
	/// `upgradeProbe` is off.
	pub fn should_use_h3(&self, url: &reqwest::Url) -> Option<u16> {
		self.confirmed_port(url).or_else(|| self.probe_candidate(url))
	}

	/// Record a foreground request's time-to-response-headers for its protocol
	/// family, and demote the origin to TCP if QUIC is provenly, sustainedly
	/// slower than TCP for it.
	///
	/// Time-to-headers includes server think-time, which varies per endpoint far
	/// more than per transport; only the averages across many requests are
	/// comparable, never individual samples — hence the minimum sample counts.
	/// Redirects followed inside the attempt inflate a sample for whichever
	/// family carried it, which the averaging absorbs the same way.
	///
	/// The comparison is deliberately asymmetric: HTTP/3 is preferred at parity
	/// and when moderately slower, because its advantages (no head-of-line
	/// blocking, connection migration) pay off beyond the mean. Only a large
	/// sustained gap demotes.
	pub fn record_path_time(&self, url: &reqwest::Url, version: http::Version, elapsed: Duration) {
		if self.slow_factor <= 0.0 {
			return;
		}

		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		let sample_ms = elapsed.as_secs_f64() * 1000.0;
		let times = if version == http::Version::HTTP_3 {
			&self.quic_times
		} else {
			&self.tcp_times
		};

		let updated = times
			.entry(origin.clone())
			.and_upsert_with(|existing| match existing {
				None => PathTime {
					avg_ms: sample_ms,
					count: 1,
				},
				Some(entry) => {
					let entry = entry.into_value();
					PathTime {
						avg_ms: entry.avg_ms * (1.0 - EWMA_ALPHA) + sample_ms * EWMA_ALPHA,
						count: entry.count.saturating_add(1),
					}
				}
			})
			.into_value();

		if version == http::Version::HTTP_3 && updated.count >= EWMA_MIN_SAMPLES {
			if let Some(tcp) = self.tcp_times.get(&origin) {
				if tcp.count >= EWMA_MIN_SAMPLES
					&& updated.avg_ms > tcp.avg_ms * self.slow_factor
					&& updated.avg_ms - tcp.avg_ms > SLOW_FLOOR_MS
				{
					self.demote_slow(&origin);
				}
			}
		}
	}

	/// Demote a working-but-slow QUIC origin back to TCP.
	///
	/// The confirmed entry moves back to `advertised` rather than being dropped:
	/// when the `slow` marker expires, the advertisement is what makes the next
	/// request trigger a re-probe — "has this path improved?" asked at zero
	/// foreground cost. The QUIC average is cleared so the answer is judged on
	/// fresh samples, not held hostage by the history that demoted it.
	fn demote_slow(&self, origin: &str) {
		let key = origin.to_string();
		let Some(entry) = self.confirmed.get(&key) else {
			return;
		};

		self.confirmed.invalidate(&key);
		self.advertised.insert(
			key.clone(),
			AltSvcEntry {
				port: entry.port,
				expires: Instant::now() + self.advertised_ttl,
			},
		);
		self.quic_times.invalidate(&key);
		self.slow.insert(key, ());
	}

	/// Record that HTTP/3 worked for this origin, on the port it connected to.
	///
	/// `port` must be the port the successful attempt actually used. Recovering it
	/// from the caches instead would be unsound: a concurrent failure that cleared
	/// them leaves nothing to read, and falling back to the origin's own port would
	/// confirm HTTP/3 on a port the server never advertised — for `confirmed_ttl`,
	/// and invisibly, since the concurrent failure's `failed` entry masks it until
	/// that expires.
	pub fn confirm_h3(&self, url: &reqwest::Url, port: u16) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		// Promoted out of `advertised`; it has served its purpose.
		self.advertised.invalidate(&origin);
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

		// This is reachable from a `Drop` impl (see the guard below), which must
		// never panic: a panic while already unwinding aborts the process. Use a
		// saturating add so an absurd `upgrade_cancel_strikes` can't overflow.
		let strikes = self
			.cancellations
			.entry(origin)
			.and_upsert_with(|existing| {
				existing.map_or(1, |entry| entry.into_value().saturating_add(1))
			})
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

pub fn parse_alt_svc_header(value: &str) -> Option<AltSvcAdvertisement> {
	if value == "clear" {
		return None;
	}

	for service in value.split(',') {
		let service = service.trim();
		if service.is_empty() {
			continue;
		}

		let mut protocol_id: Option<&str> = None;
		let mut host: Option<&str> = None;
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
					// The alt-authority is `[host]:port`, where an omitted host means
					// the origin's own. Keep the host: acting on an advertisement for
					// a different host would be the same unsupported inference as
					// acting on one for a different port.
					//
					// Split on the *last* colon so a bracketed IPv6 literal survives,
					// and keep it exactly as written — brackets included. That is the
					// form `Url::host_str` also returns for IPv6, so comparing the two
					// needs no normalising on either side.
					if let Some((alt_host, port_str)) = value.rsplit_once(':') {
						host = Some(alt_host);
						if let Ok(p) = port_str.parse::<u16>() {
							port = Some(p);
						}
					}
				}
				_ => {}
			}
		}

		if protocol_id.is_some() && port.is_some() {
			return Some(AltSvcAdvertisement {
				host: host.unwrap_or_default().to_owned(),
				port: port.unwrap(),
				max_age,
			});
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

/// Verifies advertised HTTP/3 endpoints in the background, so no foreground
/// request ever waits on an unverified QUIC path.
///
/// The probe is a real request — `HEAD /` sent with `Version::HTTP_3` — on the
/// **raw** `reqwest::Client`, not the middleware stack. That is load-bearing
/// three times over: it bypasses the HTTP cache, so a replayed cached response
/// (rebuilt with its stored HTTP version) can never fake a confirmation; it
/// bypasses [`AltSvcMiddleware`], so probing cannot recurse; and it shares the
/// h3 connection pool with foreground requests, so a successful probe leaves
/// behind a warm QUIC connection the next request rides. Confirmation doubles
/// as prewarming.
///
/// Any HTTP/3 response confirms, regardless of status: a 401 or 405 to
/// `HEAD /` proves the transport end-to-end just as well as a 200.
pub struct H3Prober {
	client: reqwest::Client,
	cache: Arc<AltSvcCache>,
	/// `None` leaves the attempt bounded only by the QUIC idle timeout.
	timeout: Option<Duration>,
	/// Handles for in-flight probes, so `Agent::close` can abort them: a probe
	/// holds a clone of the raw client, which would otherwise keep the
	/// connection pool alive past close for up to the probe timeout.
	tasks: std::sync::Mutex<Vec<tokio::task::AbortHandle>>,
}

impl std::fmt::Debug for H3Prober {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("H3Prober")
			.field("timeout", &self.timeout)
			.finish()
	}
}

impl H3Prober {
	pub fn new(client: reqwest::Client, cache: Arc<AltSvcCache>, timeout: Option<Duration>) -> Self {
		Self {
			client,
			cache,
			timeout,
			tasks: std::sync::Mutex::new(Vec::new()),
		}
	}

	/// Spawn a probe of `port` for the origin of `url`. The caller must hold the
	/// origin's single-flight claim (see [`AltSvcCache::claim_probe`]).
	fn spawn(&self, url: reqwest::Url, port: u16) {
		let client = self.client.clone();
		let cache = Arc::clone(&self.cache);
		let timeout = self.timeout;

		let handle = tokio::spawn(async move {
			let mut probe_url = url.clone();
			probe_url.set_path("/");
			probe_url.set_query(None);
			probe_url.set_fragment(None);
			let _ = probe_url.set_username("");
			let _ = probe_url.set_password(None);
			// Same rewrite rule as the foreground path: a port differing from the
			// origin's only gets here when `follow_advertised_port` is on.
			if Some(port) != url.port_or_known_default() {
				let _ = probe_url.set_port(Some(port));
			}

			let attempt = client
				.head(probe_url)
				.version(http::Version::HTTP_3)
				.send();

			let outcome = match timeout {
				Some(limit) => tokio::time::timeout(limit, attempt).await.ok(),
				None => Some(attempt.await),
			};

			// Cache operations stay keyed on `url`, the origin, matching the
			// foreground path.
			match outcome {
				Some(Ok(response)) if response.version() == http::Version::HTTP_3 => {
					cache.confirm_h3(&url, port);
				}
				// A response that is somehow not HTTP/3 is a failure too: the
				// h3 route did not deliver, whatever answered.
				_ => cache.record_h3_failure(&url),
			}

			// An aborted probe never reaches this; its claim expires on the
			// probing TTL instead, which is why that TTL exceeds the timeout.
			cache.finish_probe(&url);
		});

		// A poisoned lock only means another thread panicked mid-push; the Vec
		// is still sound to use, and probing must never take the process down.
		let mut tasks = self
			.tasks
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner);
		tasks.retain(|task| !task.is_finished());
		tasks.push(handle.abort_handle());
	}

	pub fn abort_all(&self) {
		let mut tasks = self
			.tasks
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner);
		for task in tasks.drain(..) {
			task.abort();
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
	/// `Some` routes foreground requests on confirmed origins only, verifying
	/// advertisements in the background. `None` restores the inline upgrade,
	/// where the next foreground request is the verification.
	prober: Option<Arc<H3Prober>>,
}

impl std::fmt::Debug for AltSvcMiddleware {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcMiddleware")
			.field("enabled", &self.enabled)
			.field("attempt_timeout", &self.attempt_timeout)
			.field("prober", &self.prober)
			.field("cache", &self.cache)
			.finish()
	}
}

impl AltSvcMiddleware {
	pub fn new(
		cache: Arc<AltSvcCache>,
		enabled: bool,
		attempt_timeout: Option<Duration>,
		prober: Option<Arc<H3Prober>>,
	) -> Self {
		Self {
			cache,
			enabled,
			attempt_timeout,
			prober,
		}
	}

	#[allow(dead_code)]
	pub fn cache(&self) -> &Arc<AltSvcCache> {
		&self.cache
	}

	/// Kick off a background probe for the URL's origin if one is warranted:
	/// probing enabled, an actionable advertisement present, the origin neither
	/// confirmed, failed, nor slow, and no probe already in flight.
	fn maybe_probe(&self, url: &reqwest::Url) {
		let Some(prober) = &self.prober else {
			return;
		};
		let Some(port) = self.cache.probe_candidate(url) else {
			return;
		};
		if !self.cache.claim_probe(url) {
			return;
		}
		prober.spawn(url.clone(), port);
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

		// With a prober, routing consults proven origins only — advertisements
		// get verified out-of-band, so no foreground request ever waits on an
		// unverified QUIC path. Without one, the legacy inline upgrade applies.
		let h3_route = if self.prober.is_some() {
			self.cache.confirmed_port(&url)
		} else {
			self.cache.should_use_h3(&url)
		};

		if let Some(h3_port) = h3_route {
			// Clone the request before attempting HTTP/3 so we can retry with TCP if it fails
			if let Some(req_clone) = req.try_clone() {
				*req.version_mut() = http::Version::HTTP_3;

				// A port differing from the origin's only comes back when
				// `follow_advertised_port` is set — `should_use_h3` filters
				// mismatches out otherwise. Rewriting the URL is the only way to
				// make reqwest connect elsewhere, and it MUST happen after the
				// clone above so the TCP fallback still targets the origin.
				//
				// Every cache operation below keeps using `url`, the origin, so
				// confirmations, failures and strikes stay keyed on the origin
				// rather than on the alternative endpoint.
				if Some(h3_port) != url.port_or_known_default() {
					let _ = req.url_mut().set_port(Some(h3_port));
				}

				let mut guard = H3AttemptGuard::new(Arc::clone(&self.cache), url.clone());
				// Measured to response headers: this layer sits inside the cache
				// middleware, so `next.run` resolves when headers arrive, before
				// any body buffering.
				let started = Instant::now();
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
							self.cache.confirm_h3(&url, h3_port);
							self.cache
								.record_path_time(&url, response.version(), started.elapsed());
						}

						if let Some(alt_svc) = response.headers().get("alt-svc") {
							if let Ok(value) = alt_svc.to_str() {
								if let Some(advertisement) = parse_alt_svc_header(value) {
									self.cache.record_alt_svc(&url, &advertisement);
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
						let started = Instant::now();
						let result = next.run(req_clone, extensions).await;
						if let Ok(ref response) = result {
							self.cache
								.record_path_time(&url, response.version(), started.elapsed());
						}
						result
					}
				}
			} else {
				// Can't clone request (streaming body), just proceed without HTTP/3
				next.run(req, extensions).await
			}
		} else {
			// An advertisement from an earlier response may still be waiting on
			// verification (or on a fresh single-flight claim after a probe task
			// died); this is the belt to the post-response trigger's braces.
			self.maybe_probe(&url);

			let started = Instant::now();
			let result = next.run(req, extensions).await;

			// Check for Alt-Svc header in non-HTTP/3 responses
			if let Ok(ref response) = result {
				self.cache
					.record_path_time(&url, response.version(), started.elapsed());

				if let Some(alt_svc) = response.headers().get("alt-svc") {
					if let Ok(value) = alt_svc.to_str() {
						if let Some(advertisement) = parse_alt_svc_header(value) {
							self.cache.record_alt_svc(&url, &advertisement);
							// Probe as soon as the advertisement lands, racing
							// the gap before the caller's next request.
							self.maybe_probe(&url);
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
		assert_eq!(result, Some(ad(443, Some(Duration::from_secs(86400)))));
	}

	#[test]
	fn test_parse_alt_svc_no_max_age() {
		let result = parse_alt_svc_header(r#"h3=":443""#);
		assert_eq!(result, Some(ad(443, None)));
	}

	#[test]
	fn test_parse_alt_svc_different_port() {
		let result = parse_alt_svc_header(r#"h3=":8443"; ma=3600"#);
		assert_eq!(result, Some(ad(8443, Some(Duration::from_secs(3600)))));
	}

	#[test]
	fn test_parse_alt_svc_multiple_protocols() {
		let result = parse_alt_svc_header(r#"h2=":443", h3=":443"; ma=86400"#);
		assert_eq!(result, Some(ad(443, Some(Duration::from_secs(86400)))));
	}

	#[test]
	fn test_parse_alt_svc_h3_variant() {
		let result = parse_alt_svc_header(r#"h3-29=":443"; ma=86400"#);
		assert_eq!(result, Some(ad(443, Some(Duration::from_secs(86400)))));
	}

	#[test]
	fn test_parse_alt_svc_keeps_the_host() {
		let result = parse_alt_svc_header(r#"h3="cdn.example.net:443"; ma=3600"#);
		assert_eq!(
			result,
			Some(AltSvcAdvertisement {
				host: "cdn.example.net".to_string(),
				port: 443,
				max_age: Some(Duration::from_secs(3600)),
			}),
			"the alt-authority's host must survive parsing, or a different-host \
			 advertisement looks same-host once the port matches"
		);
	}

	#[test]
	fn test_parse_alt_svc_ipv6_host() {
		let result = parse_alt_svc_header(r#"h3="[2001:db8::1]:8443""#);
		assert_eq!(
			result,
			Some(AltSvcAdvertisement {
				// Brackets kept: this is the form `Url::host_str` returns too, so the
				// two compare directly.
				host: "[2001:db8::1]".to_string(),
				port: 8443,
				max_age: None,
			}),
			"splitting on the last colon keeps a bracketed IPv6 literal intact"
		);
	}

	#[test]
	fn test_ipv6_origin_accepts_its_own_host_spelled_out() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://[2001:db8::1]/path").unwrap();

		cache.record_alt_svc(
			&url,
			&AltSvcAdvertisement {
				host: "[2001:db8::1]".to_string(),
				port: 443,
				max_age: None,
			},
		);

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"an IPv6 origin naming its own address is the same host, brackets and all"
		);
	}

	#[test]
	fn test_host_comparison_ignores_case() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(
			&url,
			&AltSvcAdvertisement {
				host: "ExAmPlE.CoM".to_string(),
				port: 443,
				max_age: None,
			},
		);

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"host names are case-insensitive, so this still names the origin's own host"
		);
	}

	#[test]
	fn test_alt_svc_on_another_host_is_not_recorded() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(
			&url,
			&AltSvcAdvertisement {
				host: "cdn.example.net".to_string(),
				port: 443,
				max_age: None,
			},
		);

		assert!(
			cache.should_use_h3(&url).is_none(),
			"h3 on another host says nothing about this one, and the port matching is \
			 coincidental"
		);
	}

	#[test]
	fn test_alt_svc_naming_our_own_host_is_recorded() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(
			&url,
			&AltSvcAdvertisement {
				host: "example.com".to_string(),
				port: 443,
				max_age: None,
			},
		);

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"spelling out the origin's own host is equivalent to omitting it"
		);
	}

	#[test]
	fn test_confirm_h3_uses_the_port_it_was_given() {
		// A concurrent failure can clear both caches between the attempt starting and
		// confirming. `confirm_h3` must not fall back to the origin's port then, or it
		// would confirm h3 on a port nobody advertised.
		let cache = test_cache_with(3, Duration::from_secs(60), true);
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(8443, None));
		cache.record_h3_failure(&url);
		cache.confirm_h3(&url, 8443);

		let entry = cache
			.confirmed
			.get(&"https://example.com:443".to_string())
			.expect("the successful attempt is confirmed");
		assert_eq!(
			entry.port, 8443,
			"confirmed on the port actually connected to, not the origin's"
		);
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

	/// A same-host advertisement, the common case.
	fn ad(port: u16, max_age: Option<Duration>) -> AltSvcAdvertisement {
		AltSvcAdvertisement {
			host: String::new(),
			port,
			max_age,
		}
	}

	fn test_cache() -> AltSvcCache {
		test_cache_with(3, Duration::from_secs(60), false)
	}

	fn test_cache_with(
		cancel_strikes: u32,
		strike_window: Duration,
		follow_advertised_port: bool,
	) -> AltSvcCache {
		AltSvcCache::new(AltSvcCacheConfig {
			advertised_ttl: Duration::from_secs(86400),
			confirmed_ttl: Duration::from_secs(86400),
			failed_ttl: Duration::from_secs(300),
			capacity: 10_000,
			cancel_strikes,
			strike_window,
			follow_advertised_port,
			probe_ttl: Duration::from_secs(10),
			slow_factor: 2.5,
			slow_ttl: Duration::from_millis(200),
		})
	}

	#[test]
	fn test_advertised_port_matching_origin_upgrades() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(443, None));

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"an advertisement for the origin's own port is actionable"
		);
	}

	#[test]
	fn test_advertised_port_mismatch_does_not_upgrade() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(8443, None));

		assert!(
			cache.should_use_h3(&url).is_none(),
			"h3 advertised on :8443 says nothing about :443, so don't upgrade"
		);
	}

	#[test]
	fn test_advertised_port_mismatch_is_still_recorded() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(8443, None));

		let entry = cache
			.advertised
			.get(&"https://example.com:443".to_string())
			.expect("the advertisement is kept even though it isn't actionable");
		assert_eq!(
			entry.port, 8443,
			"keeping it means the port is available if reqwest ever lets us honour it"
		);
	}

	#[test]
	fn test_advertised_port_mismatch_upgrades_when_following() {
		let cache = test_cache_with(3, Duration::from_secs(60), true);
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(8443, None));

		assert_eq!(
			cache.should_use_h3(&url),
			Some(8443),
			"opting in returns the advertised port so the request can be rewritten"
		);
	}

	#[test]
	fn test_cache_flow() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		assert!(cache.should_use_h3(&url).is_none());

		cache.record_alt_svc(&url, &ad(443, Some(Duration::from_secs(3600))));
		assert_eq!(cache.should_use_h3(&url), Some(443));

		cache.confirm_h3(&url, 443);
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

		cache.record_alt_svc(&url, &ad(443, None));
		assert!(cache.should_use_h3(&url).is_some());

		cache.record_h3_failure(&url);
		assert!(cache.should_use_h3(&url).is_none());

		cache.record_alt_svc(&url, &ad(443, None));
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
	fn test_hint_is_confirmed_not_probed() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.add_hint("example.com", 443);

		assert_eq!(
			cache.confirmed_port(&url),
			Some(443),
			"a hint is the user's assertion and routes immediately, probe or no probe"
		);
		assert!(
			cache.probe_candidate(&url).is_none(),
			"nothing to verify: the hint already confirmed the origin"
		);
	}

	#[test]
	fn test_advertised_routes_nothing_but_probes() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(443, None));

		assert!(
			cache.confirmed_port(&url).is_none(),
			"an advertisement is evidence worth probing, not worth routing on"
		);
		assert_eq!(
			cache.probe_candidate(&url),
			Some(443),
			"and it is exactly what the probe should verify"
		);
	}

	#[test]
	fn test_probe_candidate_respects_failed() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(443, None));
		cache.record_h3_failure(&url);

		assert!(
			cache.probe_candidate(&url).is_none(),
			"a failed origin is not re-probed until the cooldown lapses"
		);
	}

	#[test]
	fn test_probe_confirmation_promotes() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(443, None));
		assert!(cache.claim_probe(&url), "first claim wins");
		cache.confirm_h3(&url, 443);
		cache.finish_probe(&url);

		assert_eq!(cache.confirmed_port(&url), Some(443));
		assert!(
			cache.probe_candidate(&url).is_none(),
			"confirmed origins are not probed again"
		);
	}

	#[test]
	fn test_claim_probe_is_single_flight() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		assert!(cache.claim_probe(&url));
		assert!(
			!cache.claim_probe(&url),
			"a second claim while one is in flight loses"
		);

		cache.finish_probe(&url);
		assert!(
			cache.claim_probe(&url),
			"finishing the probe frees the origin for the next one"
		);
	}

	#[test]
	fn test_slow_demotion_needs_sustained_evidence() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(443, None));
		cache.confirm_h3(&url, 443);

		// Plenty of TCP samples at 5ms, but too few QUIC samples to act on.
		for _ in 0..EWMA_MIN_SAMPLES {
			cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(5));
		}
		for _ in 0..(EWMA_MIN_SAMPLES - 1) {
			cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(50));
		}

		assert_eq!(
			cache.confirmed_port(&url),
			Some(443),
			"below the minimum sample count no comparison may act"
		);
	}

	#[test]
	fn test_slow_demotion_moves_origin_back_to_probing() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(443, None));
		cache.confirm_h3(&url, 443);

		// TCP steady at 5ms, QUIC steady at 50ms: 10x the average and 45ms over,
		// clearing both the factor and the absolute floor.
		for _ in 0..EWMA_MIN_SAMPLES {
			cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(5));
			cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(50));
		}

		assert!(
			cache.confirmed_port(&url).is_none(),
			"a sustained large gap demotes the origin off HTTP/3"
		);
		assert!(
			cache.probe_candidate(&url).is_none(),
			"while the slow marker lives, the origin is not re-probed either"
		);
		assert!(
			!cache
				.failed
				.contains_key(&"https://example.com:443".to_string()),
			"slow is not broken: the failed cache stays out of it"
		);

		// The test cache's slow TTL is short; once it lapses, the advertisement
		// preserved by the demotion re-enters through a probe.
		std::thread::sleep(Duration::from_millis(300));
		assert_eq!(
			cache.probe_candidate(&url),
			Some(443),
			"slow expiry re-enters via the probe, asking whether the path improved"
		);
	}

	#[test]
	fn test_parity_or_moderately_slower_quic_is_kept() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();

		cache.record_alt_svc(&url, &ad(443, None));
		cache.confirm_h3(&url, 443);

		// QUIC 2x slower and 20ms over: above the floor but below the 2.5x
		// factor, so HTTP/3's structural advantages win the tie.
		for _ in 0..(EWMA_MIN_SAMPLES * 2) {
			cache.record_path_time(&url, http::Version::HTTP_2, Duration::from_millis(20));
			cache.record_path_time(&url, http::Version::HTTP_3, Duration::from_millis(40));
		}

		assert_eq!(
			cache.confirmed_port(&url),
			Some(443),
			"moderately slower QUIC is still preferred"
		);
	}

	#[test]
	fn test_cancellation_below_threshold_keeps_h3() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, &ad(443, None));

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
		cache.record_alt_svc(&url, &ad(443, None));

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
		cache.record_alt_svc(&url, &ad(443, None));

		cache.record_h3_cancellation(&url);
		cache.record_h3_cancellation(&url);
		cache.confirm_h3(&url, 443);
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
		let cache = test_cache_with(0, Duration::from_secs(60), false);
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, &ad(443, None));

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
		let cache = test_cache_with(3, Duration::from_millis(50), false);
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, &ad(443, None));

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
