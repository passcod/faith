use std::{sync::Arc, time::Duration};

use crate::cache::AltSvcCache;

/// Verifies advertised HTTP/3 endpoints in the background, so no foreground
/// request ever waits on an unverified QUIC path.
///
/// The probe is a real request — `HEAD /` sent with `Version::HTTP_3` — on the
/// **raw** `reqwest::Client`, not the middleware stack. That is load-bearing
/// three times over: it bypasses the HTTP cache, so a replayed cached response
/// (rebuilt with its stored HTTP version) can never fake a confirmation; it
/// bypasses [`AltSvcMiddleware`](crate::AltSvcMiddleware), so probing cannot recurse; and it shares the
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
	pub fn new(
		client: reqwest::Client,
		cache: Arc<AltSvcCache>,
		timeout: Option<Duration>,
	) -> Self {
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

			let attempt = client.head(probe_url).version(http::Version::HTTP_3).send();

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

	/// Kick off a background probe for the URL's origin if one is warranted: an actionable
	/// advertisement present, the origin neither confirmed, failed, nor slow, and no probe
	/// already in flight. The same decision the Alt-Svc layer makes on a request, exposed so a
	/// `preconnect` TCP warm-up to a probe-worthy origin triggers a probe as a real request would.
	pub fn maybe_probe(&self, url: &reqwest::Url) {
		let Some(port) = self.cache.probe_candidate(url) else {
			return;
		};
		if !self.cache.claim_probe(url) {
			return;
		}
		self.spawn(url.clone(), port);
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

/// Feeds `HTTPS` DNS records into the upgrade layer, so an origin advertising `alpn="h3"` is
/// probe-worthy from its first request rather than from the first TCP response carrying an
/// `Alt-Svc` header.
///
/// Installed on the resolver by the agent (see [`web_faith_dns::FaithResolver::set_https_sink`]),
/// which is the only place that holds all three: the resolver is built before the cache, and the
/// prober holds a client that holds the resolver, so nothing lower down can own this.
///
/// The record is read at the bare name, which per RFC 9460 is the record for the origin at the
/// default HTTPS port; the resolver sees only a hostname, so that is also the only origin it could
/// name. Recording it there is right whichever request triggered the lookup, because what the
/// record describes does not depend on who asked.
// spec:H3UP#advertisements-from-dns
// spec:DNS#https-records
pub struct H3HttpsSink {
	cache: Arc<AltSvcCache>,
	/// Weak, and load-bearingly so: the prober holds the client, the client holds the resolver,
	/// and the resolver holds this sink. A strong reference here would close that ring and leak
	/// the whole graph — connection pool included — past `Agent::close`, which works by dropping
	/// the client. The agent owns the only strong reference, so this lives exactly as long as the
	/// agent's prober does.
	///
	/// `None` rather than a dead handle when probing is off, where an advertisement is acted on
	/// inline by the next foreground request instead.
	// spec:PROBE
	prober: Option<std::sync::Weak<H3Prober>>,
}

impl std::fmt::Debug for H3HttpsSink {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("H3HttpsSink")
			.field("probing", &self.prober.is_some())
			.finish()
	}
}

impl H3HttpsSink {
	pub fn new(cache: Arc<AltSvcCache>, prober: Option<&Arc<H3Prober>>) -> Self {
		Self {
			cache,
			prober: prober.map(Arc::downgrade),
		}
	}

	/// The origin a record at `host` describes: the default HTTPS port, which is the port whose
	/// record lives at the bare name.
	fn origin_url(host: &str) -> Option<reqwest::Url> {
		reqwest::Url::parse(&format!("https://{host}")).ok()
	}
}

impl web_faith_dns::HttpsSink for H3HttpsSink {
	fn wants(&self, host: &str) -> bool {
		Self::origin_url(host).is_some_and(|url| self.cache.wants_https_record(&url))
	}

	fn record(&self, host: &str, advertisement: web_faith_dns::HttpsAdvertisement) {
		let Some(url) = Self::origin_url(host) else {
			return;
		};

		self.cache
			.record_https_record(&url, advertisement.port, advertisement.ttl);

		// Probe straight away rather than waiting for the request that triggered the lookup to
		// finish: the point of reading DNS is that the path can be verified while that request is
		// still on TCP, so the one after it upgrades.
		//
		// A prober that has gone means the agent was closed (or rebuilt) while this query was in
		// flight; the advertisement above is still worth keeping, but there is nothing left to
		// probe it with, and resurrecting a dropped client to try would be exactly wrong.
		if let Some(prober) = self.prober.as_ref().and_then(std::sync::Weak::upgrade) {
			prober.maybe_probe(&url);
		}
	}
}
