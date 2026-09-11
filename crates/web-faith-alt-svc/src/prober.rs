//! Background HTTP/3 probing.
use std::{sync::Arc, time::Duration};

use crate::cache::AltSvcCache;

/// Verifies advertised HTTP/3 endpoints in the background, so no foreground request waits on an
/// unverified QUIC path.
///
/// The probe is a real `HEAD /` at `Version::HTTP_3`, sent on the **raw** client rather than the
/// middleware stack. That bypasses the HTTP cache, so a replayed cached response cannot fake a
/// confirmation; bypasses [`AltSvcMiddleware`](crate::AltSvcMiddleware), so probing cannot recurse;
/// and shares the h3 pool, so a successful probe leaves a warm connection behind.
///
/// Any HTTP/3 response confirms whatever its status: a 401 or 405 proves the transport as well as
/// a 200 does.
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
	/// A prober sending on `client`, reporting into `cache`.
	///
	/// `timeout` bounds one probe; `None` leaves it bounded only by the QUIC idle timeout.
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

	/// Abort every probe in flight, so none outlives the client it sends on.
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
