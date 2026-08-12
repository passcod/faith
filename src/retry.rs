//! Retrying a request whose pooled connection died before the origin answered.
//!
//! An origin that closes idle connections aggressively hands the pool a problem it
//! cannot see: the response was complete and said nothing, so the connection goes
//! back in looking healthy, and the close lands afterwards. A request written into
//! it in the meantime is lost, and without this layer the caller sees a network
//! error for a request the origin never received.

use http::{Extensions, Method};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next, Result};

/// How many times a request may be replayed before the failure reaches the caller.
///
/// More than one, because the pool does not hold just one dead connection. An origin
/// closing idle connections closes all of them, so the pool comes back with several
/// that are already gone, and a replay draws from that same pool: it can pick another
/// dead one. Each attempt that fails has at least consumed and evicted one of them,
/// so the replays needed are bounded by how many the pool was holding rather than by
/// anything about the origin. Measured against the conformance dimension, one replay
/// leaves 40% of requests failing and four leave none; five is that with room.
///
/// It is a safety valve rather than a tuning knob. An origin that really does end
/// every connection this way, fresh ones included, is indistinguishable from a pool
/// full of dead connections -- nothing in the error says whether it was reused -- so
/// this bounds what such an origin costs. The request's own timeout is the outer
/// backstop; this keeps a broken origin from being handed six connection attempts
/// per request for longer than it takes to learn the answer.
const MAX_REPLAYS: usize = 5;

/// Replays a request when the connection ended before any response arrived.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeadConnectionRetry;

/// Whether replaying the request cannot change what the origin ends up having done.
///
/// The safety argument is entirely about the method, and deliberately not about
/// whether a response arrived. "No response bytes came back" does not mean the
/// origin did not process the request: an origin that half-closes goes on reading
/// and handling requests it can no longer answer, and one that dies after committing
/// its work looks the same from here as one that never saw the request. Nothing in
/// the error distinguishes those from a connection that was already gone, so a
/// request that must not happen twice is not retried at all.
fn is_idempotent(method: &Method) -> bool {
	matches!(
		*method,
		Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE | Method::PUT | Method::DELETE
	)
}

/// Whether the error is a connection that ended before a complete response arrived.
///
/// Narrower than "the request failed": a refused connection, a TLS failure or a
/// timeout are all real answers about the origin, and replaying them would double
/// the work done on the way to the same result.
///
/// The whole source chain is walked rather than the outermost error inspected,
/// because the classification sits several layers below the one this sees.
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
