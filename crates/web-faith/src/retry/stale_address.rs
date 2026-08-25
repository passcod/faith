use http::Extensions;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result};
use web_faith_dns::FaithResolver;

use super::failed_to_connect;

/// Re-resolves and attempts a request again when connecting to a stale-served address failed.
///
/// Serving an expired DNS answer trades a round trip against the chance the address has moved, and
/// this layer is what bounds the cost of being wrong to one re-resolve rather than a failed request.
// spec:DNS#when-a-stale-address-is-wrong
#[derive(Debug, Clone)]
pub struct StaleAddressRetry {
	/// `None` under `dns.system: true`, where Faith holds no cache and so serves nothing stale.
	resolver: Option<FaithResolver>,
}

impl StaleAddressRetry {
	pub fn new(resolver: Option<FaithResolver>) -> Self {
		Self { resolver }
	}
}

#[async_trait::async_trait]
impl Middleware for StaleAddressRetry {
	// spec:DNS#when-a-stale-address-is-wrong
	async fn handle(
		&self,
		req: Request,
		extensions: &mut Extensions,
		next: Next<'_>,
	) -> Result<Response> {
		let Some(resolver) = self.resolver.clone() else {
			return next.run(req, extensions).await;
		};

		// Asked before the request runs, not after it fails. The lookup this request makes serves the
		// stale entry and starts a refresh behind it, so by the time an error is in hand the entry may
		// already have been replaced and the question "was this address assumed?" no longer answerable.
		//
		// Cloned here for the same reason the dead-connection layer clones early: sending consumes the
		// body. A `ReadableStream` body does not clone, which is what leaves those requests reporting
		// the connect failure rather than being attempted again -- the body has no second copy, whether
		// or not it was read.
		let host = req.url().host_str().map(str::to_owned);
		let replay = match &host {
			Some(host) if resolver.served_stale(host) => req.try_clone(),
			_ => None,
		};

		let outcome = next.clone().run(req, extensions).await;

		match &outcome {
			Err(err) if failed_to_connect(err) => {}
			_ => return outcome,
		}

		let (Some(host), Some(request)) = (host, replay) else {
			return outcome;
		};

		// Drop the entry so the retry's lookup waits for a fresh answer rather than being served the
		// address that just failed. One attempt only: the fresh address is confirmed rather than
		// assumed, so a second failure is an answer about the origin and belongs to the caller.
		resolver.invalidate_stale(&host);
		next.run(request, extensions).await
	}
}
