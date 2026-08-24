//! What an agent counts about the requests it has run.

use std::sync::atomic::{AtomicU64, Ordering};

/// The agent's running counters, incremented as requests and bodies pass through it.
///
/// Held behind an `Arc` and shared with every response the agent produces, since a body finishing
/// is what settles two of these and the response is what knows it happened.
#[derive(Debug, Default)]
pub struct InnerAgentStats {
	pub requests_sent: AtomicU64,
	pub responses_received: AtomicU64,
	pub bodies_started: AtomicU64,
	pub bodies_finished: AtomicU64,
}

impl InnerAgentStats {
	/// Read the counters as they stand.
	pub fn snapshot(&self) -> AgentStats {
		AgentStats {
			requests_sent: self.requests_sent.load(Ordering::Relaxed),
			responses_received: self.responses_received.load(Ordering::Relaxed),
			bodies_started: self.bodies_started.load(Ordering::Relaxed),
			bodies_finished: self.bodies_finished.load(Ordering::Relaxed),
		}
	}
}

/// A reading of an agent's counters, taken at one moment.
///
/// Non-exhaustive: what an agent counts can grow, and a new counter should not be a breaking change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentStats {
	pub requests_sent: u64,
	pub responses_received: u64,
	/// Response body streams that have been started, which is what reading a body does.
	pub bodies_started: u64,
	/// Response body streams that have been read to the end. While more have started than
	/// finished, that many bodies are holding connections open.
	pub bodies_finished: u64,
}
