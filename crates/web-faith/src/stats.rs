//! The agent's request counters.

use std::sync::atomic::{AtomicU64, Ordering};

/// The live counters, shared with every response the agent produces: a body finishing is what
/// settles two of them.
#[derive(Debug, Default)]
pub(crate) struct InnerAgentStats {
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

/// Statistics gathered by the agent.
///
/// A snapshot taken when [`Agent::stats`](crate::Agent::stats) is called; it does not update live.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentStats {
	pub requests_sent: u64,
	pub responses_received: u64,
	/// Response body streams that have been started, which is what reading a body does.
	pub bodies_started: u64,
	/// Response body streams read to the end. The gap against `bodies_started` is how many bodies
	/// are holding a connection open.
	pub bodies_finished: u64,
}
