//! The process's revocation data, shared by every agent.
//!
//! One [`Revocation`] per process: the first agent that uses a source creates it, with Mozilla's
//! data, the platform's per-user cache directory, and a client of its own for the poller. Later
//! agents share it.

// spec:REVOC

use std::{
	sync::{Arc, OnceLock},
	time::Duration,
};

use web_faith_revocation::{Refusals, Revocation, RevocationConfig, Source, Sources};

use crate::{
	USER_AGENT,
	client::{NodeEnvRecipe, TlsRecipe, crypto_provider},
	error::{FaithError, FaithErrorKind},
};

/// Generous next to what a fetch takes, since the largest is a few megabytes of CRLite filter, but
/// finite.
const POLLER_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const POLLER_READ_TIMEOUT: Duration = Duration::from_secs(60);
const POLLER_TOTAL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

static REVOCATION: OnceLock<Revocation> = OnceLock::new();

/// What an agent checks, and where its refusals are counted.
#[derive(Debug, Clone)]
pub(crate) struct RevocationRecipe {
	pub(crate) handle: Revocation,
	pub(crate) sources: Sources,
	pub(crate) refusals: Arc<Refusals>,
}

impl RevocationRecipe {
	/// Start the agent's sources, reading each one's cache now if this is the first agent to use
	/// it. `None` when the agent uses no source.
	// spec:REVOC#the-cache
	pub(crate) fn start(sources: Sources) -> Result<Option<Self>, FaithError> {
		if sources.iter().next().is_none() {
			return Ok(None);
		}

		let handle = handle()?;
		for source in sources.iter() {
			handle.enable(source);
		}
		Ok(Some(Self {
			handle: handle.clone(),
			sources,
			refusals: Arc::new(Refusals::default()),
		}))
	}

	/// The agent's stats for `source`: `None` for a source it does not use.
	pub(crate) fn stats(&self, source: Source) -> Option<crate::stats::RevocationStats> {
		self.sources
			.iter()
			.any(|on| on == source)
			.then(|| crate::stats::RevocationStats {
				refused: self.refusals.count(source),
				last_confirmed: self.handle.last_confirmed(source),
			})
	}
}

fn handle() -> Result<&'static Revocation, FaithError> {
	if let Some(handle) = REVOCATION.get() {
		return Ok(handle);
	}

	let mut config = RevocationConfig::mozilla(crypto_provider());
	// spec:ENV#revocation-cache
	config.cache_dir = directories::BaseDirs::new()
		.map(|dirs| dirs.cache_dir().join("web-faith").join("revocation"));
	let revocation = Revocation::new(config, poller_client()?);
	// Two agents racing to be first both build one; whichever lands is the one every agent shares.
	Ok(REVOCATION.get_or_init(|| revocation))
}

/// The poller's client: the environment's network settings and none of any agent's options.
///
/// It carries timeouts of its own: the poller is one loop awaiting each fetch, so a connection that
/// stalls would otherwise stop every later check for the life of the process.
///
/// Its connections are not checked for revocation themselves. What it fetches is protected by the
/// content signature, and checking the connection against the data it is fetching adds nothing.
// spec:REVOC#keeping-current
fn poller_client() -> Result<reqwest::Client, FaithError> {
	let mut node_env = NodeEnvRecipe::read();
	// The data is signed, so the poller has no use for a switch that turns verification off.
	node_env.accept_invalid_certs = false;

	let tls = TlsRecipe::default().config(&node_env)?;
	node_env
		.apply(
			reqwest::Client::builder()
				.tls_backend_preconfigured(tls)
				.connect_timeout(POLLER_CONNECT_TIMEOUT)
				.read_timeout(POLLER_READ_TIMEOUT)
				.timeout(POLLER_TOTAL_TIMEOUT)
				.user_agent(USER_AGENT),
		)
		.build()
		.map_err(|err| FaithError::new(FaithErrorKind::Config, format!("{err:?}")))
}

/// Point the process's revocation checking at `config` instead of Mozilla's. It must be called
/// before any agent in the process starts checking, and returns whether it took effect.
///
/// Permanently unstable: this exists for tests that stand up their own Remote Settings server.
#[cfg(feature = "unstable-internals")]
pub fn install(config: RevocationConfig) -> bool {
	let Ok(client) = poller_client() else {
		return false;
	};
	let mut config = Some(config);
	REVOCATION.get_or_init(|| Revocation::new(config.take().expect("taken once"), client));
	config.is_none()
}
