//! The agent's warm-up verbs.

// spec:WARM

use std::{future::Future, sync::atomic::Ordering};

use moka::sync::Cache as MokaCache;
use reqwest::Version;

use crate::{
	agent::Agent,
	error::{FaithError, FaithErrorKind},
	warm_up::{extract_host, origin_key, reduce_to_origin},
};

impl Agent {
	/// Warm the DNS cache for `host`, so a later request to it skips the lookup.
	///
	/// Takes a bare host; a scheme, port or path is ignored. The future completes when the answer
	/// lands and never fails — the work is advisory — and does nothing under the system resolver,
	/// which has no cache to warm. A host with nothing to resolve, or a closed agent, is refused
	/// here rather than by the future.
	// spec:WARM
	pub fn prefetch_dns(&self, host: &str) -> Result<impl Future<Output = ()> + use<>, FaithError> {
		if self.is_closed() {
			return Err(FaithErrorKind::Closed.into());
		}

		let Some(host) = extract_host(host) else {
			return Err(FaithErrorKind::AddressParse.into());
		};

		#[cfg(feature = "dns")]
		let resolver = self.dns_resolver();
		Ok(async move {
			// Nothing to warm without Faith's own resolver: the platform's cache is not ours to fill.
			#[cfg(feature = "dns")]
			if let Some(resolver) = resolver {
				resolver.prefetch(&host).await;
			}
			#[cfg(not(feature = "dns"))]
			let _ = host;
		})
	}

	/// Open a pooled connection to `origin`, so the first request to it skips DNS, TCP and TLS
	/// setup.
	///
	/// Takes an origin (`scheme://host[:port]`); a longer URL is reduced to one. Sends a synthetic
	/// `HEAD` to the origin's root — which the origin will see in its logs — over the transport the
	/// next request would use. The future completes when the attempt finishes and never fails.
	/// Something unconnectable, or a closed agent, is refused here rather than by the future.
	// spec:WARM
	pub fn preconnect(&self, origin: &str) -> Result<impl Future<Output = ()> + use<>, FaithError> {
		let Some(raw_client) = self.raw_client() else {
			return Err(FaithErrorKind::Closed.into());
		};

		let Some(url) = reduce_to_origin(origin) else {
			return Err(FaithErrorKind::AddressParse.into());
		};
		let key = origin_key(&url);

		// Already warm within the idle window, or a warm-up for this origin already in flight:
		// either way there is no new work to do, so finish without opening a duplicate.
		let redundant = self.warmed.contains_key(&key)
			|| !self.warming.entry(key.clone()).or_insert(()).is_fresh();

		// The transport the next foreground request would take, decided exactly as the Alt-Svc
		// layer decides it: nothing upgrades with the machinery off; with a prober, only a
		// confirmed origin routes to QUIC (an advertisement is evidence worth probing, not worth
		// routing on); without one, the inline upgrade acts on advertisements too. Diverging here
		// would warm the wrong transport.
		// spec:WARM#preconnect
		#[cfg(feature = "http3")]
		let h3_port = self
			.alt_svc_cache()
			.filter(|_| self.h3_upgrade_enabled)
			.and_then(|cache| {
				if self.h3_prober().is_some() {
					cache.confirmed_port(&url)
				} else {
					cache.should_use_h3(&url)
				}
			});
		#[cfg(not(feature = "http3"))]
		let h3_port: Option<u16> = None;

		#[cfg(feature = "connection-tracking")]
		let conn_tracker = self.conn_tracker.clone();
		let warmed = self.warmed.clone();
		let warming = self.warming.clone();
		// Read before the warm-up starts, to compare against once it finishes.
		let warm_generation = self.warm_generation.clone();
		let generation = warm_generation.load(Ordering::Relaxed);

		Ok(async move {
			if redundant {
				return;
			}

			// Release the single-flight claim whatever happens, so a later warm-up is not blocked
			// by this one having finished.
			struct ReleaseClaim {
				warming: MokaCache<String, ()>,
				key: String,
			}
			impl Drop for ReleaseClaim {
				fn drop(&mut self) {
					self.warming.invalidate(&self.key);
				}
			}
			let _release = ReleaseClaim {
				warming,
				key: key.clone(),
			};

			let request = match h3_port {
				Some(port) => {
					let mut h3_url = url.clone();
					// A port differing from the origin's only comes back with the
					// follow-advertised-port option on; rewriting the URL is how reqwest is told to
					// connect there, mirroring the foreground path.
					if Some(port) != h3_url.port_or_known_default() {
						let _ = h3_url.set_port(Some(port));
					}
					raw_client.head(h3_url).version(Version::HTTP_3)
				}
				None => raw_client.head(url.clone()),
			};

			let outcome = request.send().await;

			// A TCP warm-up leaves a pooled connection to track; a QUIC one does not (QUIC
			// connections are not tracked, and a confirmed origin has nothing left to probe).
			#[cfg(feature = "connection-tracking")]
			if h3_port.is_none()
				&& let Ok(response) = &outcome
				&& let Some(info) = response
					.extensions()
					.get::<hyper_util::client::legacy::connect::HttpInfo>()
			{
				conn_tracker.track_warmup(info.local_addr(), info.remote_addr());
			}

			// A network change while this was in flight leaves the origin unmarked: the connection
			// landed in the pool that change dropped, so it is not warm however well the request
			// went.
			// spec:NETCHG#reach-across-the-subsystems
			if outcome.is_ok() && warm_generation.load(Ordering::Relaxed) == generation {
				warmed.insert(key, ());
			}
		})
	}
}
