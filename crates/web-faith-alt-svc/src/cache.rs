use std::time::{Duration, Instant};

use moka::sync::Cache;

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

/// A run of consecutive HTTP/3 failures against one origin.
///
/// Both instants are carried in the value rather than left to the cache's TTL,
/// because they differ per origin and from each other: the entry deliberately
/// outlives the cooldown it set, so that a count survives the block it caused and
/// can escalate the next one. `advertised` does the same for `ma`.
// spec:H3UP#failure-backoff
#[derive(Debug, Clone, Copy)]
struct FailureEntry {
	/// Consecutive failures with no confirmation in between.
	count: u32,
	/// Until when the origin is blocked from upgrading, probing, and recording
	/// advertisements. The only field that gates behaviour.
	blocked_until: Instant,
	/// Until when `count` still describes a run. Past it the origin is judged
	/// from the base cooldown again.
	counted_until: Instant,
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
pub const SLOW_FLOOR_MS: f64 = 10.0;

pub struct AltSvcCacheConfig {
	pub advertised_ttl: Duration,
	pub confirmed_ttl: Duration,
	/// Cooldown a first failure earns; each consecutive one doubles it.
	pub failed_ttl: Duration,
	/// Ceiling on the doubling. Clamped up to `failed_ttl`, so setting it at or
	/// below the base gives a flat cooldown.
	pub failed_max_ttl: Duration,
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
	/// Origins that failed over HTTP/3, with their run of consecutive failures.
	/// An entry present here is not necessarily blocked: see [`Self::is_failed`].
	failed: Cache<String, FailureEntry>,
	/// Consecutive cancelled HTTP/3 attempts per origin. Entries expire on a TTL
	/// (the strike window), so a run has to be sustained to count.
	cancellations: Cache<String, u32>,
	/// Single-flight claims for in-flight background probes.
	probing: Cache<String, ()>,
	/// Origins demoted for being slower over QUIC than over TCP. Distinct from
	/// `failed`: the path *works*, so re-advertisements must not be discarded,
	/// and expiry re-enters through a probe rather than treating h3 as broken.
	slow: Cache<String, ()>,
	/// Origins seeded from `http3.hints`, with the port hinted. A hint is the
	/// caller's assertion rather than something observed, so it has to be
	/// distinguishable from an entry in `confirmed` that a real HTTP/3 response
	/// put there: [`Self::network_changed`] demotes the observed ones and
	/// re-seeds from here. Unbounded by TTL and outside the capacity bound,
	/// because the hints are configuration and there are as many as the caller
	/// passed.
	// spec:NETCHG#what-the-signal-keeps
	hints: Cache<String, u16>,
	/// Time-to-headers over TCP (h1 and h2 together), per origin.
	tcp_times: Cache<String, PathTime>,
	/// Time-to-headers over QUIC (h3), per origin.
	quic_times: Cache<String, PathTime>,

	advertised_ttl: Duration,
	confirmed_ttl: Duration,
	failed_ttl: Duration,
	failed_max_ttl: Duration,
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
			.field("hint_count", &self.hints.entry_count())
			.finish()
	}
}

impl AltSvcCache {
	pub fn new(config: AltSvcCacheConfig) -> Self {
		let AltSvcCacheConfig {
			advertised_ttl,
			confirmed_ttl,
			failed_ttl,
			failed_max_ttl,
			capacity,
			cancel_strikes,
			strike_window,
			follow_advertised_port,
			probe_ttl,
			slow_factor,
			slow_ttl,
		} = config;

		// A cap below the base would mean the first failure already exceeds it;
		// clamping makes that setting a flat cooldown rather than a shorter one.
		let failed_max_ttl = failed_max_ttl.max(failed_ttl);

		Self {
			advertised: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(advertised_ttl)
				.build(),
			confirmed: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(confirmed_ttl)
				.build(),
			// Twice the longest cooldown: the outer bound on how long an entry
			// can be worth keeping, since a count is dropped one cooldown after
			// the block it caused lapsed. Per-entry instants do the real work.
			failed: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(failed_max_ttl.saturating_mul(2))
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
			// No TTL and no capacity bound: hints are configuration, held for the
			// life of the agent so a network change can re-seed from them.
			hints: Cache::builder().build(),
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
			failed_ttl,
			failed_max_ttl,
			cancel_strikes,
			follow_advertised_port,
			slow_factor,
		}
	}

	/// The cooldown the `count`-th consecutive failure earns: the base doubled
	/// once per failure before it, capped.
	// spec:H3UP#failure-backoff
	fn failure_cooldown(&self, count: u32) -> Duration {
		let doublings = count.saturating_sub(1).min(u32::BITS - 1);
		self.failed_ttl
			.saturating_mul(2u32.saturating_pow(doublings))
			.min(self.failed_max_ttl)
	}

	/// Whether the origin is inside its failure cooldown.
	///
	/// Presence in `failed` is not the question: an entry outlives its cooldown
	/// so the failure count survives to escalate the next one.
	fn is_failed(&self, origin: &str) -> bool {
		self.failed
			.get(origin)
			.is_some_and(|entry| entry.blocked_until > Instant::now())
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

		if self.is_failed(&origin) {
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

	/// Whether an `HTTPS` DNS record for this origin would tell us anything we do not already
	/// know, so the resolver can skip the query rather than send one per lookup.
	///
	/// Nothing is learnable while the origin is confirmed (already routing over HTTP/3), failed
	/// (blocked whatever a record says), slow (demoted on measurement, which a record cannot
	/// overturn), or already carrying a live advertisement (the probe it warrants is already
	/// warranted). Each of those states expires, and the query resumes when it does.
	// spec:DNS#https-records
	pub fn wants_https_record(&self, url: &reqwest::Url) -> bool {
		let Some(origin) = Self::origin_key(url) else {
			return false;
		};

		!self.is_failed(&origin)
			&& !self.confirmed.contains_key(&origin)
			&& !self.slow.contains_key(&origin)
			&& self
				.advertised
				.get(&origin)
				.is_none_or(|entry| entry.expires <= Instant::now())
	}

	/// Record an HTTP/3 advertisement carried by an `HTTPS` DNS record.
	///
	/// An `HTTPS` record and an `Alt-Svc` header are two ways for an origin to say the same thing,
	/// so this lands in exactly the state a header advertisement does: the origin becomes
	/// probe-worthy, and foreground requests keep to TCP until a probe proves the path. The port
	/// and same-host rules are the header's too — [`Self::record_alt_svc`] applies them — because
	/// the reasons for them are about what Faith can connect to rather than about where the
	/// advertisement was read.
	// spec:H3UP#advertisements-from-dns
	pub fn record_https_record(&self, url: &reqwest::Url, port: Option<u16>, ttl: Duration) {
		// A record naming no port describes the origin's own, exactly as an `Alt-Svc` header with
		// no alt-authority port would.
		let Some(port) = port.or_else(|| url.port_or_known_default()) else {
			return;
		};

		self.record_alt_svc(
			url,
			&AltSvcAdvertisement {
				// The same-host case: a record targeting another host is dropped before it gets
				// here, since Faith only upgrades to the origin's own host.
				host: String::new(),
				port,
				// The record's own DNS TTL is how long what it says is good for, which is the
				// role `ma` plays for a header advertisement.
				max_age: Some(ttl),
			},
		);
	}

	/// Hints seed `confirmed` directly, not `advertised`: a hint is the *user's*
	/// assertion, and routing it through a probe would both second-guess an
	/// explicit instruction and break h3-only origins (no TCP listener), which
	/// only work if the very first request speaks HTTP/3. Distrust is reserved
	/// for what servers advertise. Failure demotes a hinted origin exactly as it
	/// does a confirmed one.
	pub fn add_hint(&self, host: &str, port: u16) {
		let origin = format!("https://{}:{}", host, port);

		// Recorded whether or not it can be acted on right now: the hint is
		// configuration, and a failure blocking it is a fact about a path that a
		// network change can clear (spec:NETCHG#what-the-signal-keeps).
		self.hints.insert(origin.clone(), port);
		self.seed_hint(origin, port);
	}

	/// Put a hinted origin into `confirmed`, unless a failure currently blocks it.
	///
	/// Split out of [`Self::add_hint`] so [`Self::network_changed`] can re-seed the
	/// hints it just cleared without re-recording them.
	fn seed_hint(&self, origin: String, port: u16) {
		if self.is_failed(&origin) {
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

		if self.is_failed(&origin) || self.slow.contains_key(&origin) {
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

		if self.is_failed(&origin)
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
	/// probing is off.
	pub fn should_use_h3(&self, url: &reqwest::Url) -> Option<u16> {
		self.confirmed_port(url)
			.or_else(|| self.probe_candidate(url))
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

		if version == http::Version::HTTP_3
			&& updated.count >= EWMA_MIN_SAMPLES
			&& let Some(tcp) = self.tcp_times.get(&origin)
			&& tcp.count >= EWMA_MIN_SAMPLES
			&& updated.avg_ms > tcp.avg_ms * self.slow_factor
			&& updated.avg_ms - tcp.avg_ms > SLOW_FLOOR_MS
		{
			self.demote_slow(&origin);
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
		// A working h3 response is proof of health; forget any strikes, and end
		// whatever run of failures preceded it.
		self.cancellations.invalidate(&origin);
		self.clear_failure_count(&origin);

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

	/// Forget the origin's run of failures, so the next one starts the backoff
	/// from the base cooldown again.
	///
	/// A cooldown still running is left alone. A confirmation racing a concurrent
	/// failure must not unblock the origin that failure just blocked: the failure
	/// is the more recent evidence about the path, and [`Self::confirm_h3`]
	/// relies on its own entry being masked until the block lapses.
	// spec:H3UP#failure-backoff
	fn clear_failure_count(&self, origin: &str) {
		let Some(entry) = self.failed.get(origin) else {
			return;
		};

		if entry.blocked_until > Instant::now() {
			self.failed
				.insert(origin.to_string(), FailureEntry { count: 0, ..entry });
		} else {
			self.failed.invalidate(origin);
		}
	}

	/// Discard everything this cache learned by observing the network, keeping
	/// what it was told.
	///
	/// Every state here except `advertised` and `hints` describes the path between
	/// this client and an origin, and a network change is exactly the event that
	/// invalidates such a description. So the observation-confirmed origins are
	/// demoted rather than kept (the path that proved them is gone, and a probe
	/// re-proves them without a foreground request paying for it), and the
	/// failures, strikes, slow markers and averages go entirely: they are
	/// penalties and measurements the old path earned, and carrying them over
	/// would judge the new network by the old one's behaviour.
	///
	/// What the origin said about itself (`advertised`) and what the caller
	/// asserted (`hints`) are not observations, so both survive.
	// spec:NETCHG
	pub fn network_changed(&self) {
		let now = Instant::now();

		// Demote first, while `confirmed` still holds the entries: an advertisement
		// is what makes the next request to the origin trigger a re-probe.
		//
		// Keys are invalidated one by one rather than with `invalidate_all`, whose
		// timestamp-based invalidation would race the hint re-seeding below.
		for (origin, entry) in self.confirmed.iter() {
			// A hint holds its origin confirmed; it is an assertion, not a finding.
			if self.hints.contains_key(origin.as_str()) {
				continue;
			}

			self.confirmed.invalidate(origin.as_str());

			// A logically expired entry is not knowledge to carry forward: it would
			// come back as a fresh advertisement having just lapsed as a confirmation.
			if entry.expires <= now {
				continue;
			}

			self.advertised.insert(
				(*origin).clone(),
				AltSvcEntry {
					port: entry.port,
					expires: now + self.advertised_ttl,
				},
			);
		}

		self.failed.invalidate_all();
		self.cancellations.invalidate_all();
		self.slow.invalidate_all();
		// In-flight probes are aborted by the caller of this method, so their
		// single-flight claims would otherwise hold their origins until the claim
		// TTL lapsed.
		self.probing.invalidate_all();
		self.tcp_times.invalidate_all();
		self.quic_times.invalidate_all();

		// After the failures are cleared, so a hint that a cooldown had been
		// blocking takes effect now rather than staying refused.
		for (origin, port) in self.hints.iter() {
			self.seed_hint((*origin).clone(), port);
		}
	}

	/// Record a failed HTTP/3 attempt, blocking the origin for a cooldown that
	/// lengthens the longer it keeps failing.
	// spec:H3UP#failure-backoff
	pub fn record_h3_failure(&self, url: &reqwest::Url) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		self.advertised.invalidate(&origin);
		self.confirmed.invalidate(&origin);
		// Already demoted; further counting is meaningless.
		self.cancellations.invalidate(&origin);

		let now = Instant::now();
		// An entry whose run has lapsed is history, not a run in progress: the
		// origin went a whole further cooldown without failing again, so it is
		// judged from the base.
		let count = self
			.failed
			.get(&origin)
			.filter(|entry| entry.counted_until > now)
			.map_or(1, |entry| entry.count.saturating_add(1));
		let cooldown = self.failure_cooldown(count);

		self.failed.insert(
			origin,
			FailureEntry {
				count,
				blocked_until: now + cooldown,
				// The count has to outlive the block it caused, or it could never
				// escalate: the next attempt only comes once the block lapses.
				counted_until: now + cooldown.saturating_mul(2),
			},
		);
	}
}

#[cfg(test)]
mod tests;
