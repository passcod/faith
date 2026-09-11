//! The agent builder's terminal.
//!
//! `bon` generates the builder and its setters from `AgentOptions`, so an option is declared once,
//! as a field, rather than once as a field and again as a setter. What it does not generate is a
//! finisher that yields an `Agent`: its own is `into_options`, which yields the options.

// spec:AGENT

use crate::{agent::Agent, error::FaithError, options::agent_options_builder};

pub use crate::options::AgentOptionsBuilder;

impl<S: agent_options_builder::State> AgentOptionsBuilder<S> {
	/// Validate what has been set and build the agent.
	pub fn build(self) -> Result<Agent, FaithError> {
		Agent::from_options_impl(self.into_options_inner())
	}
}

/// Taking the options rather than the agent. Permanently unstable.
#[cfg(feature = "internals")]
impl<S: agent_options_builder::IsComplete> AgentOptionsBuilder<S> {
	/// The options as they stand, for passing on rather than building an agent here.
	pub fn into_options(self) -> crate::options::AgentOptions {
		self.into_options_inner()
	}
}
