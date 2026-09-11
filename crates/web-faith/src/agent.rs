//! The agent and its builder.

pub use crate::builder::AgentOptionsBuilder;
pub use crate::stats::AgentStats;

#[cfg(feature = "cache")]
pub use crate::options::{CacheOptions, CacheStore};
pub use crate::options::{
	DnsOptions, DnsOverride, FlowControlOptions, Header, Http2Options, PoolOptions, QuirksOptions,
	RedirectPolicy, TimeoutOptions, TlsOptions,
};
#[cfg(feature = "http3")]
pub use crate::options::{Http3Congestion, Http3Hint, Http3Options};

// spec:AGENT spec:WARM spec:NETCHG spec:OBS

use std::sync::{
	Arc, RwLock,
	atomic::{AtomicU64, Ordering},
};

use moka::sync::Cache as MokaCache;
use reqwest::Client;
use reqwest_middleware::ClientWithMiddleware;
use url::Url;

#[cfg(feature = "encoding")]
use http::header::HeaderValue;

#[cfg(feature = "connection-tracking")]
use web_faith_conn_tracker::{ConnectionSnapshot, ConnectionTracker};

#[cfg(feature = "cookies")]
use web_faith_cookies::FaithJar;

#[cfg(feature = "dns")]
use web_faith_dns::{FaithResolver, ResolverReport};

#[cfg(feature = "http3")]
use web_faith_alt_svc::{AltSvcCache, H3Prober};

use crate::{client::ClientRecipe, stats::InnerAgentStats, warm_up::origin_key};

#[cfg(all(feature = "http3", feature = "dns"))]
use crate::client::install_https_sink;

mod build;
mod warm;

#[cfg(test)]
mod tests;

/// The agent settings a request consults, as opposed to those a client is built from.
#[derive(Debug, Clone, Default)]
pub(crate) struct AgentSettings {
	/// Whether an HTTP/3 upgrade may follow a port the origin advertised, which a request needs so
	/// a rewritten port is not reported as a redirect.
	pub(crate) h3_follow_advertised_port: bool,
	/// Whether a streaming request body may go out over HTTP/1.x.
	pub(crate) quirk_h1_request_streaming: bool,
	/// The agent's default `Accept-Encoding`. Decides which codings a response is decoded under
	/// when a request adds none of its own.
	#[cfg(feature = "encoding")]
	pub(crate) default_accept_encoding: Option<HeaderValue>,
	/// The agent's default `Content-Encoding`. A request layers its own coding on top of this.
	#[cfg(feature = "encoding")]
	pub(crate) default_content_encoding: Option<HeaderValue>,
	/// Whether a `Priority` header sits among the agent's default headers, so that default wins
	/// over the one a request's priority would derive.
	pub(crate) has_default_content_type: bool,
	pub(crate) has_default_priority: bool,
}

/// The resources an agent holds while open, and gives up when closed.
///
/// Behind a shared lock so closing acts on the agent rather than the handle it was called through.
#[derive(Debug)]
pub(crate) struct Live {
	/// Holds the connection pool, DNS resolver and background tasks, so dropping it releases them.
	pub(crate) client: ClientWithMiddleware,
	/// The raw client behind [`Self::client`], sharing its pool. A warm-up sends here to skip the
	/// HTTP cache and the Alt-Svc layer while still pooling the connection.
	// spec:WARM
	pub(crate) raw_client: Client,
	/// The DNS resolver, shared with the client so a prefetch warms the cache requests read. `None`
	/// under the system resolver, where there is no such cache.
	// spec:WARM
	#[cfg(feature = "dns")]
	pub(crate) dns_resolver: Option<FaithResolver>,
	#[cfg(feature = "http3")]
	pub(crate) alt_svc_cache: Option<Arc<AltSvcCache>>,
	/// Held so closing can abort in-flight probes; each owns a clone of the raw client, which would
	/// otherwise keep the pool alive past close for up to the probe timeout.
	#[cfg(feature = "http3")]
	pub(crate) h3_prober: Option<Arc<H3Prober>>,
}

/// A Faith HTTP agent: where all fetches start.
///
/// An agent holds the resources and state shared across requests — connection pool, caches, DNS
/// resolver, cookie jar, HTTP/3 upgrade memory — and is the browser instance of this library. A
/// typical application makes one and starts every request from it.
///
/// [`Agent::new`] takes the defaults; [`Agent::builder`] configures one.
// spec:AGENT
#[derive(Debug, Clone)]
pub struct Agent {
	/// `None` once [`Agent::close`] has been called.
	live: Arc<RwLock<Option<Live>>>,
	/// Origins with a warm-up connection opened within the pool idle window, so a repeat
	/// warm-up does no new work. Keyed by `scheme://host:port`; entries expire with the idle
	/// timeout.
	// spec:WARM
	pub(crate) warmed: MokaCache<String, ()>,
	/// Single-flight claims for warm-ups in flight, so concurrent calls for the same
	/// origin do not open duplicate connections.
	// spec:WARM
	pub(crate) warming: MokaCache<String, ()>,
	/// Bumped by [`Self::network_changed`], so a warm-up in flight across the signal does not record
	/// its origin as warm — its connection went into the pool that was just dropped.
	// spec:NETCHG#reach-across-the-subsystems
	pub(crate) warm_generation: Arc<AtomicU64>,
	/// The jar outlives a close and stays readable from a closed agent.
	#[cfg(feature = "cookies")]
	pub(crate) cookie_jar: Option<Arc<FaithJar>>,
	pub(crate) stats: Arc<InnerAgentStats>,
	#[cfg(feature = "connection-tracking")]
	pub(crate) conn_tracker: Arc<ConnectionTracker>,
	/// Whether an upgrade may follow a port the origin advertised. A request needs it to stop a
	/// rewritten port from being reported as a redirect.
	pub(crate) h3_follow_advertised_port: bool,
	/// Whether the upgrade machinery is on at all. A warm-up needs it to route the way a foreground
	/// request would: with it off, nothing upgrades, whatever the caches hold.
	// spec:WARM#preconnect
	#[cfg(feature = "http3")]
	pub(crate) h3_upgrade_enabled: bool,
	/// Whether a streaming request body may go out over HTTP/1.x, which the fetch standard otherwise
	/// reserves to HTTP/2 and HTTP/3.
	// spec:QUIRK#http-1-x-request-body-streaming
	pub(crate) quirk_h1_request_streaming: bool,
	/// The agent's default `Accept-Encoding`. Decides the codings a response is decoded under when
	/// a request adds none of its own.
	#[cfg(feature = "encoding")]
	pub(crate) default_accept_encoding: Option<HeaderValue>,
	/// The agent's default `Content-Encoding`. A request layers its own coding on top of this.
	// spec:ENC
	#[cfg(feature = "encoding")]
	pub(crate) default_content_encoding: Option<HeaderValue>,
	/// Whether a `Content-Type` sits among the agent's default headers. A type the agent declares
	/// describes the bodies its requests carry, so it wins over the one a body's kind implies.
	// spec:REQ#body
	pub(crate) has_default_content_type: bool,
	/// Whether a `Priority` header sits among the agent's default headers. That default wins over
	/// the header a request's priority would derive.
	pub(crate) has_default_priority: bool,
	/// How to build this agent's clients, so [`Self::network_changed`] can build them again.
	// spec:NETCHG
	pub(crate) recipe: Arc<ClientRecipe>,
}

impl Agent {
	/// The agent's cookie jar, if it keeps one.
	///
	/// The jar itself, so cookies go in and out through the type `web-faith-cookies` documents. It
	/// stays readable after [`Self::close`].
	// spec:COOK
	#[cfg(feature = "cookies")]
	pub fn cookies(&self) -> Option<&Arc<FaithJar>> {
		self.cookie_jar.as_ref()
	}

	/// The client this agent sends through, or `None` once it is closed.
	///
	/// A request takes its handle when it is issued, which lets one already in flight
	/// finish while a later one is refused.
	// spec:AGENT
	#[cfg(feature = "raw-client")]
	pub fn client(&self) -> Option<ClientWithMiddleware> {
		self.live().as_ref().map(|live| live.client.clone())
	}

	// spec:AGENT
	#[cfg(not(feature = "raw-client"))]
	pub(crate) fn client(&self) -> Option<ClientWithMiddleware> {
		self.live().as_ref().map(|live| live.client.clone())
	}

	/// The same client without Faith's middleware, so a request on it skips the HTTP cache and the
	/// Alt-Svc layer while sharing the connection pool.
	#[cfg(feature = "raw-client")]
	pub fn raw_client(&self) -> Option<Client> {
		self.live().as_ref().map(|live| live.raw_client.clone())
	}

	#[cfg(not(feature = "raw-client"))]
	pub(crate) fn raw_client(&self) -> Option<Client> {
		self.live().as_ref().map(|live| live.raw_client.clone())
	}

	/// The agent's DNS resolver, or `None` once it is closed.
	#[cfg(all(feature = "dns", feature = "raw-client"))]
	pub fn dns_resolver(&self) -> Option<FaithResolver> {
		self.dns_resolver_inner()
	}

	#[cfg(feature = "dns")]
	pub(crate) fn dns_resolver_inner(&self) -> Option<FaithResolver> {
		self.live()
			.as_ref()
			.and_then(|live| live.dns_resolver.clone())
	}

	#[cfg(feature = "http3")]
	fn alt_svc_cache(&self) -> Option<Arc<AltSvcCache>> {
		self.live()
			.as_ref()
			.and_then(|live| live.alt_svc_cache.clone())
	}

	#[cfg(feature = "http3")]
	fn h3_prober(&self) -> Option<Arc<H3Prober>> {
		self.live().as_ref().and_then(|live| live.h3_prober.clone())
	}

	fn live(&self) -> std::sync::RwLockReadGuard<'_, Option<Live>> {
		self.live
			.read()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
	}

	fn live_mut(&self) -> std::sync::RwLockWriteGuard<'_, Option<Live>> {
		self.live
			.write()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
	}

	/// Close the agent, releasing its connection pool, DNS resolver, and background tasks without
	/// waiting for the last clone to drop. Worth doing if you make many short-lived agents.
	///
	/// Requests already in flight run to completion. A request issued on a closed agent fails with
	/// [`FaithErrorKind::Closed`](crate::error::FaithErrorKind::Closed). Calling it more than once is a
	/// no-op, and the cookie jar, if any, stays readable through `cookies()`.
	pub fn close(&self) {
		// Dropping the client releases the reqwest connection pool and the
		// Hickory resolver task; the alt-svc cache goes with it. The raw client
		// shares that pool and the resolver, so it goes too, and both are what a
		// later warm-up checks to refuse with the closed-agent error.
		// Taken out of the shared cell, so every handle on this agent sees it closed.
		let Some(live) = self.live_mut().take() else {
			return;
		};

		// Probes hold a raw client clone; abort them so the pool doesn't outlive close by up to
		// the probe timeout.
		#[cfg(feature = "http3")]
		if let Some(prober) = &live.h3_prober {
			prober.abort_all();
		}

		drop(live);
	}

	/// Tell the agent the network under it has changed, so it stops acting on what it learned
	/// about a network that is gone.
	///
	/// There is no portable signal for an interface or connectivity change, so call this yourself
	/// on whatever trigger fits — an OS notification, a VPN transition, a captive-portal sign-in.
	///
	/// Drops pooled connections, flushes the DNS cache, demotes confirmed HTTP/3 origins back to
	/// advertised so a probe re-verifies them, and clears the HTTP/3 failure, slow and path-time
	/// state. Configuration, `http3.hints`, `Alt-Svc` advertisements, the cookie jar, the HTTP
	/// cache and the counters are kept — none of those is a claim about a network path.
	///
	/// Requests in flight run to completion on the connections they hold. Harmless to call
	/// repeatedly, or on a closed agent.
	// spec:NETCHG
	pub fn network_changed(&self) {
		{
			// Held across the rebuild so a close cannot land halfway through it.
			let mut guard = self.live_mut();
			// A closed agent has already released all of this.
			let Some(live) = guard.as_mut() else {
				return;
			};

			// reqwest cannot drop pooled connections short of dropping the client, so the client is
			// rebuilt from the recipe the agent kept for this. Requests in flight hold the handle
			// they took when they were issued, so they run to completion and the old pool goes when
			// the last of them finishes.
			//
			// A rebuild that fails leaves the agent on its existing client: the options were already
			// validated at construction, so a failure here is not the caller's to answer for, and an
			// agent that still works on the old network beats one that works nowhere.
			let built = self.recipe.build(
				#[cfg(feature = "cookies")]
				self.cookie_jar.as_ref(),
				#[cfg(feature = "dns")]
				live.dns_resolver.as_ref(),
				#[cfg(feature = "http3")]
				live.alt_svc_cache.as_ref(),
			);
			if let Ok(built) = built {
				#[cfg(feature = "http3")]
				{
					// Abort probes running on the old client: each holds a clone of it, and their
					// answers would describe the path that has just gone away.
					if let Some(prober) = &live.h3_prober {
						prober.abort_all();
					}
					live.h3_prober = built.prober;
					// The sink holds the prober, which has just been replaced along with the client
					// it sends on; leaving the old one installed would aim DNS-triggered probes at a
					// client that has been dropped.
					#[cfg(feature = "dns")]
					install_https_sink(
						live.dns_resolver.as_ref(),
						live.alt_svc_cache.as_ref(),
						live.h3_prober.as_ref(),
						self.h3_upgrade_enabled,
					);
				}
				live.client = built.client;
				live.raw_client = built.raw_client;
			}

			// Names resolve afresh against the new network, through that network's own servers: the
			// resolver drops what it read off the old one and reads again when next used. Under the
			// system resolver there is no resolver here and so nothing to reset.
			// spec:DNS
			#[cfg(feature = "dns")]
			if let Some(resolver) = &live.dns_resolver {
				resolver.reset();
			}

			#[cfg(feature = "http3")]
			if let Some(alt_svc_cache) = &live.alt_svc_cache {
				alt_svc_cache.network_changed();
			}
		}

		// The warm-up records describe pooled connections that have just been dropped, so a
		// `preconnect` after the signal opens a connection rather than finding the origin warm
		// (spec:NETCHG, spec:WARM). The single-flight claims are left alone: a warm-up still in
		// flight is not duplicated by releasing its claim, and the generation bump is what stops
		// it recording an origin as warm on the strength of a connection in the dropped pool.
		self.warmed.invalidate_all();
		self.warm_generation.fetch_add(1, Ordering::Relaxed);
	}

	/// The agent's counters, as they stand.
	pub fn stats(&self) -> AgentStats {
		self.stats.snapshot()
	}

	/// The connections this agent currently holds open.
	///
	/// TCP only; QUIC connections are not visible here. Statistics refresh once a second, so sample
	/// over time for rates such as retransmissions. Which fields are filled depends on the platform:
	/// the lost-packet count and delivery rate are Linux-only, an unsupported platform reports an
	/// empty list, and no field is guaranteed to stay available.
	#[cfg(feature = "connection-tracking")]
	pub fn connections(&self) -> Vec<ConnectionSnapshot> {
		self.conn_tracker.snapshot()
	}

	/// The DNS servers this agent resolves through, in query order.
	///
	/// Each entry gives the server's address, its transport (`udp`, `tcp`, `tls`, `https`, `quic`,
	/// or `h3`), and whether that was `configured` or `conventional`. Empty until the resolver has
	/// been used, since it reads its configuration on first use, and empty under the system
	/// resolver.
	// spec:OBS#resolvers
	#[cfg(feature = "dns")]
	pub fn resolvers(&self) -> Vec<ResolverReport> {
		self.dns_resolver_inner()
			.as_ref()
			.map(FaithResolver::resolvers)
			.unwrap_or_default()
	}

	/// Note that a request reached this origin, so a `preconnect` for it has no new work to do.
	///
	/// Called for foreground requests as well as warm-ups: the criterion is that the origin holds
	/// an idle pooled connection, not how it came to.
	// spec:WARM
	pub(crate) fn mark_warm(&self, url: &Url) {
		self.warmed.insert(origin_key(url), ());
	}

	/// Whether [`Self::close`] has been called.
	pub fn is_closed(&self) -> bool {
		self.live().is_none()
	}
}
