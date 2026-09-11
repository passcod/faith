//! Request replay.
//!
//! Two of those, each with its own layer: a pooled connection that died before the origin
//! answered, and an address from an expired DNS entry that has moved.

use http::{Extensions, Method};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result};

#[cfg(feature = "dns")]
mod stale_address;

#[cfg(feature = "dns")]
pub use stale_address::StaleAddressRetry;

// An origin closing idle connections closes all of them, so a replay can draw another dead one
// from the same pool. Against the conformance dimension, one replay left 40% of requests failing
// and four left none.
const MAX_REPLAYS: usize = 5;

/// Replays a request when the connection ended before any response arrived.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeadConnectionRetry;

/// Whether replaying the request cannot change what the origin ends up having done.
///
/// Decided on the method alone, never on whether a response arrived: an origin that half-closes
/// goes on handling requests it can no longer answer, and nothing in the error tells that apart
/// from a connection that was already gone.
fn is_idempotent(method: &Method) -> bool {
	// `QUERY` belongs here despite carrying a body: it is safe and idempotent, its query content
	// travelling in the body rather than the URL. It is compared by name because `http` has no
	// constant for a method still in draft (spec:POOL#reusing-a-connection-that-has-died).
	matches!(
		*method,
		Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE | Method::PUT | Method::DELETE
	) || method.as_str() == crate::request::QUERY
}

/// Whether the error is a connection that ended before a complete response arrived.
///
/// Narrower than "the request failed": a refused connection, a TLS failure or a timeout are all
/// answers about the origin. Walks the whole source chain, the classification sitting several
/// layers below this one.
fn died_before_response(err: &reqwest_middleware::Error) -> bool {
	let mut source: Option<&(dyn std::error::Error + 'static)> = Some(err);
	while let Some(err) = source {
		if let Some(err) = err.downcast_ref::<hyper::Error>() {
			// `is_incomplete_message` is the connection ending mid-exchange, which
			// is what an origin closing an idle connection under a reused request
			// produces. `is_closed` is the same situation caught a moment earlier,
			// where the send half was already known to be gone.
			if err.is_incomplete_message() || err.is_closed() {
				return true;
			}
		}
		source = err.source();
	}
	false
}

#[async_trait::async_trait]
impl Middleware for DeadConnectionRetry {
	// spec:POOL#reusing-a-connection-that-has-died
	async fn handle(
		&self,
		req: Request,
		extensions: &mut Extensions,
		next: Next<'_>,
	) -> Result<Response> {
		// Cloned before the request is sent, not after it fails: sending consumes the
		// body, so by the time the error is in hand there is nothing left to replay.
		// `try_clone` returns `None` for a streaming body, which is what keeps those
		// unretryable -- the stream has already been read and cannot be read again.
		let mut replay = is_idempotent(req.method())
			.then(|| req.try_clone())
			.flatten();

		let mut outcome = next.clone().run(req, extensions).await;

		for _ in 0..MAX_REPLAYS {
			match &outcome {
				Err(err) if died_before_response(err) => {}
				// Anything else is an answer about the origin -- a success, a refused
				// connection, a TLS failure, a timeout -- and replaying it would only
				// double the work done on the way to the same result.
				_ => return outcome,
			}
			let Some(request) = replay.take() else {
				return outcome;
			};
			// Re-cloned for the attempt after this one, before this one consumes it.
			replay = request.try_clone();
			outcome = next.clone().run(request, extensions).await;
		}
		outcome
	}
}

/// Whether the error is a connection that was never established.
///
/// Only a connect failure leaves open the possibility that the address was wrong; a failure the
/// origin took part in confirms the address by definition.
#[cfg(feature = "dns")]
pub(super) fn failed_to_connect(err: &reqwest_middleware::Error) -> bool {
	match err {
		reqwest_middleware::Error::Reqwest(err) => err.is_connect(),
		// A middleware's own error is about this stack rather than about the network.
		reqwest_middleware::Error::Middleware(_) => false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn idempotent_methods_are_the_replayable_ones() {
		for method in [
			Method::GET,
			Method::HEAD,
			Method::OPTIONS,
			Method::TRACE,
			Method::PUT,
			Method::DELETE,
			Method::from_bytes(b"QUERY").unwrap(),
		] {
			assert!(is_idempotent(&method), "{method} should be replayable");
		}

		// POST and PATCH are the ones a retry could double up, and CONNECT is not a
		// request this layer has any business replaying.
		for method in [Method::POST, Method::PATCH, Method::CONNECT] {
			assert!(!is_idempotent(&method), "{method} should not be replayable");
		}
	}
}
