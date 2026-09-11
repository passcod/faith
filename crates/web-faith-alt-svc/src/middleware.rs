//! The HTTP/3 upgrade layer.
use std::{
	marker::PhantomData,
	sync::Arc,
	time::{Duration, Instant},
};

use http::Extensions;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result};

use crate::{cache::AltSvcCache, header::parse_alt_svc_header, prober::H3Prober};

/// Recording the moment a response's headers arrived.
///
/// The stamp belongs to the client, which puts it in the request's extensions; this layer is only
/// the place that observes the arrival.
pub trait ArrivalStamp: Send + Sync + 'static {
	fn mark(&self, at: Instant);
}

/// Records a cancellation if the HTTP/3 attempt it guards is dropped before
/// producing an outcome.
///
/// [`AltSvcMiddleware`] can only learn that HTTP/3 is broken from the attempt's
/// return value, and a cancelled request never produces one: a caller racing
/// the request against a deadline or an abort signal drops the losing future.
/// Without this guard nothing ever demotes the origin, so a caller whose
/// deadline is shorter than the network's own failure detection re-attempts
/// HTTP/3 over a dead path on every retry, indefinitely.
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

/// The Alt-Svc layer, which upgrades an origin to HTTP/3 once it advertises one.
///
/// `S` is the client's arrival stamp, which this marks when a response's headers land; see
/// [`ArrivalStamp`].
#[derive(Clone)]
pub struct AltSvcMiddleware<S: ArrivalStamp> {
	cache: Arc<AltSvcCache>,
	enabled: bool,
	/// Ceiling on how long an HTTP/3 attempt may take to produce response
	/// headers before it is treated as failed and retried over TCP.
	attempt_timeout: Option<Duration>,
	/// `Some` routes foreground requests on confirmed origins only, verifying
	/// advertisements in the background. `None` restores the inline upgrade,
	/// where the next foreground request is the verification.
	prober: Option<Arc<H3Prober>>,
	stamp: PhantomData<fn(S)>,
}

impl<S: ArrivalStamp> std::fmt::Debug for AltSvcMiddleware<S> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcMiddleware")
			.field("enabled", &self.enabled)
			.field("attempt_timeout", &self.attempt_timeout)
			.field("prober", &self.prober)
			.field("cache", &self.cache)
			.finish()
	}
}

impl<S: ArrivalStamp> AltSvcMiddleware<S> {
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
			stamp: PhantomData,
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
		if let Some(prober) = &self.prober {
			prober.maybe_probe(url);
		}
	}
}

/// Run the rest of the stack and stamp the moment the response headers arrive.
///
/// The one place a response's arrival is observed, so the path-time average and the surfaced
/// timing read the same instant.
// spec:RESP#request-timing
async fn run_stamped<S: ArrivalStamp>(
	next: Next<'_>,
	req: Request,
	extensions: &mut Extensions,
) -> (Result<Response>, Instant) {
	let result = next.run(req, extensions).await;
	let at = Instant::now();
	if result.is_ok()
		&& let Some(stamp) = extensions.get::<S>()
	{
		stamp.mark(at);
	}
	(result, at)
}

#[async_trait::async_trait]
impl<S: ArrivalStamp> Middleware for AltSvcMiddleware<S> {
	async fn handle(
		&self,
		mut req: Request,
		extensions: &mut Extensions,
		next: Next<'_>,
	) -> Result<Response> {
		if !self.enabled {
			return run_stamped::<S>(next, req, extensions).await.0;
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
					Some(limit) => {
						tokio::time::timeout(limit, run_stamped::<S>(next.clone(), req, extensions))
							.await
							.ok()
					}
					None => Some(run_stamped::<S>(next.clone(), req, extensions).await),
				};
				// Reached on success, error and expiry alike; only a mid-flight
				// drop skips it and leaves the guard armed.
				guard.disarm();

				match outcome {
					Some((Ok(response), at)) => {
						if response.version() == http::Version::HTTP_3 {
							self.cache.confirm_h3(&url, h3_port);
							self.cache.record_path_time(
								&url,
								response.version(),
								at.duration_since(started),
							);
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
					Some((Err(_), _)) | None => {
						self.cache.record_h3_failure(&url);

						// Use the cloned request (which still has default HTTP version)
						let started = Instant::now();
						let (result, at) = run_stamped::<S>(next, req_clone, extensions).await;
						if let Ok(ref response) = result {
							self.cache.record_path_time(
								&url,
								response.version(),
								at.duration_since(started),
							);
						}
						result
					}
				}
			} else {
				// Can't clone request (streaming body), just proceed without HTTP/3
				run_stamped::<S>(next, req, extensions).await.0
			}
		} else {
			// An advertisement from an earlier response may still be waiting on
			// verification (or on a fresh single-flight claim after a probe task
			// died); this is the belt to the post-response trigger's braces.
			self.maybe_probe(&url);

			let started = Instant::now();
			let (result, at) = run_stamped::<S>(next, req, extensions).await;

			// Check for Alt-Svc header in non-HTTP/3 responses
			if let Ok(ref response) = result {
				self.cache
					.record_path_time(&url, response.version(), at.duration_since(started));

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
