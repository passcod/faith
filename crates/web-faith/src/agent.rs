//! The agent: what owns a connection pool, and the verbs that act on a live one.

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

use crate::{
	client::ClientRecipe,
	stats::{AgentStats, InnerAgentStats},
	warm_up::origin_key,
};

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
	/// The agent's default `Accept-Encoding`, if one sits among its default headers, which decides
	/// which codings a response is decoded under when a request adds none of its own.
	#[cfg(feature = "encoding")]
	pub(crate) default_accept_encoding: Option<HeaderValue>,
	/// The agent's default `Content-Encoding`, if one sits among its default headers, which a
	/// request layers its own coding on top of rather than displacing.
	#[cfg(feature = "encoding")]
	pub(crate) default_content_encoding: Option<HeaderValue>,
	/// Whether a `Priority` header sits among the agent's default headers, so that default wins
	/// over the one a request's priority would derive.
	pub(crate) has_default_priority: bool,
}

/// What an agent holds while it is open, and gives up when it is closed.
///
/// Behind a shared lock because closing acts on the agent rather than on the handle it was called
/// through: every clone names the same one, so every clone sees the result.
#[derive(Debug)]
pub(crate) struct Live {
	/// The heavy resources (connection pool, DNS resolver, background tasks) live inside this
	/// client, so dropping it is what actually releases them.
	pub(crate) client: ClientWithMiddleware,
	/// The raw `reqwest::Client` underlying [`Self::client`], sharing its connection pool. A warm-up
	/// sends its synthetic request here rather than through the middleware stack, which bypasses the
	/// HTTP cache and the Alt-Svc layer, and so keeps the warm-up out of request accounting, while
	/// still pooling the connection foreground requests reuse.
	// spec:WARM
	pub(crate) raw_client: Client,
	/// The DNS resolver, shared with the client so a prefetch warms the cache requests read. `None`
	/// under the system resolver, where there is no such cache.
	// spec:WARM
	#[cfg(feature = "dns")]
	pub(crate) dns_resolver: Option<FaithResolver>,
	#[cfg(feature = "http3")]
	pub(crate) alt_svc_cache: Option<Arc<AltSvcCache>>,
	/// Held so closing can abort in-flight background probes: each one owns a clone of the raw
	/// client, which would otherwise keep the connection pool alive past close for up to the probe
	/// timeout.
	#[cfg(feature = "http3")]
	pub(crate) h3_prober: Option<Arc<H3Prober>>,
}

/// An HTTP client with its own connection pool, caches, and resolver.
///
/// Cloning one is cheap and every clone names the same underlying agent, so cloning is how a request
/// gets an agent to run on rather than a way to get a second pool. Because clones share, closing
/// acts on the agent itself and every handle to it sees the result.
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
	/// Bumped by [`Self::network_changed`], so a warm-up that was in flight across the signal does
	/// not record its origin as warm: its connection went into the pool that was just dropped.
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
	/// The agent's default `Accept-Encoding`, if one sits among its default headers, which decides
	/// the codings a response is decoded under when a request adds none of its own.
	#[cfg(feature = "encoding")]
	pub(crate) default_accept_encoding: Option<HeaderValue>,
	/// The agent's default `Content-Encoding`, if one sits among its default headers. A request
	/// layers its own coding on top of this rather than displacing it.
	// spec:ENC
	#[cfg(feature = "encoding")]
	pub(crate) default_content_encoding: Option<HeaderValue>,
	/// Whether a `Priority` header sits among the agent's default headers. That default wins over
	/// the header a request's priority would derive.
	pub(crate) has_default_priority: bool,
	/// How to build this agent's clients, so [`Self::network_changed`] can build them again. Shared
	/// rather than cloned per handle: every handle builds the same client from the same recipe.
	// spec:NETCHG
	pub(crate) recipe: Arc<ClientRecipe>,
}

impl Agent {
	/// The agent's cookie jar, if it keeps one.
	///
	/// The jar itself, rather than per-cookie methods wrapped around it, so cookies go in and out
	/// through the type `web-faith-cookies` documents. It outlives a close and stays readable from a
	/// closed agent.
	// spec:COOK
	#[cfg(feature = "cookies")]
	pub fn cookies(&self) -> Option<&Arc<FaithJar>> {
		self.cookie_jar.as_ref()
	}

	/// Take a handle on the client, or `None` once the agent is closed.
	///
	/// A request takes its own handle at the moment it is issued, which is what lets one already in
	/// flight finish while a later one is refused.
	// spec:AGENT
	pub fn client(&self) -> Option<ClientWithMiddleware> {
		self.live().as_ref().map(|live| live.client.clone())
	}

	/// Take a handle on the raw client a warm-up sends through, or `None` once closed.
	pub fn raw_client(&self) -> Option<Client> {
		self.live().as_ref().map(|live| live.raw_client.clone())
	}

	/// The DNS resolver, if the agent has one of its own and is still open.
	#[cfg(feature = "dns")]
	pub fn dns_resolver(&self) -> Option<FaithResolver> {
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

	/// Build an agent from options, validating them into the recipe its clients are built from.
	///

	/// Close the agent, releasing its connection pool, DNS resolver, and any
	/// background tasks it owns, rather than waiting for the garbage collector
	/// to drop it. This is worth doing when you create many short-lived agents;
	/// a single long-lived agent can just be left to the GC.
	///
	/// Requests already in flight run to completion. Any new request on a closed
	/// agent throws a `Closed` error. Calling `close()` more than once is a
	/// no-op. The cookie jar, if any, remains readable through [`Self::cookies`].
	pub fn close(&self) {
		// Dropping the client releases the reqwest connection pool and the
		// Hickory resolver task; the alt-svc cache goes with it. The raw client
		// shares that pool and the resolver, so it goes too, and both are what a
		// later warm-up checks to refuse with the closed-agent error.
		// Taken out of the shared cell, so every handle on this agent sees it closed.
		let Some(live) = self.live_mut().take() else {
			return;
		};

		#[cfg(feature = "http3")]
		// Probes hold a raw client clone; abort them so the pool doesn't outlive close by up to
		// the probe timeout.
		if let Some(prober) = &live.h3_prober {
			prober.abort_all();
		}

		drop(live);
	}

	/// Tell the agent the network underneath it has changed, so it stops deciding from what it
	/// learned about a network that is gone.
	///
	/// Node has no portable signal for an interface or connectivity change, so Faith cannot
	/// detect one; this is the reaction, and wiring it to a trigger (an OS notification, a VPN
	/// transition, a captive-portal sign-in) is the caller's own. It drops pooled connections,
	/// flushes the DNS cache, demotes the HTTP/3 origins that a real response confirmed back to
	/// advertised so a background probe re-verifies them, and clears the HTTP/3 failure and slow
	/// states, their cooldown backoff, and the path-time averages.
	///
	/// Configuration, `http3.hints`, `Alt-Svc` advertisements, the cookie jar, the HTTP cache and
	/// the `stats()` counters are all kept: none of them is a claim about a network path.
	///
	/// Requests already in flight are not interrupted and run to completion on the connections
	/// they hold; the reset shapes what requests started afterwards draw on. Calling it on a
	/// closed agent does nothing, and calling it repeatedly is harmless.
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

	/// The counters this agent has gathered, as they stand.
	pub fn stats(&self) -> AgentStats {
		self.stats.snapshot()
	}

	/// Returns information on current connections open by this agent.
	///
	/// Only tracks TCP connections currently (upstream limitation). Stats are updated once a second:
	/// this makes it possible to track indicators over time to find the retransmission rate, for
	/// example. The lost-packet count and delivery rate are only available on Linux. Some other
	/// fields might also be missing depending on platform support; and no forward guarantees are made
	/// on field availability. If the platform isn't supported at all, this will always return empty.
	#[cfg(feature = "connection-tracking")]
	pub fn connections(&self) -> Vec<ConnectionSnapshot> {
		self.conn_tracker.snapshot()
	}

	/// Returns the DNS servers this agent resolves through, in the order they are queried, so
	/// "are my lookups actually encrypted" is answerable from inside the process.
	///
	/// Each entry gives the server's address, the transport in use (`udp`, `tcp`, `tls`, `https`,
	/// `quic`, or `h3`), and how that transport was arrived at (`configured` or `conventional`).
	/// The list is empty until the resolver has been used, because it reads its configuration on
	/// first use, and empty for an agent using the system resolver.
	// spec:OBS#resolvers
	#[cfg(feature = "dns")]
	pub fn resolvers(&self) -> Vec<ResolverReport> {
		self.dns_resolver()
			.as_ref()
			.map(FaithResolver::resolvers)
			.unwrap_or_default()
	}

	/// Note that a request reached this origin, so it holds a connection the pool keeps idle for
	/// the idle window and a `preconnect` for it has no new work to do.
	///
	/// Called for foreground requests as well as warm-ups, because the criterion is about the
	/// origin holding an idle pooled connection, not about how it came to hold one.
	// spec:WARM
	pub fn mark_warm(&self, url: &Url) {
		self.warmed.insert(origin_key(url), ());
	}

	/// Whether [`Self::close`] has been called.
	pub fn is_closed(&self) -> bool {
		self.live().is_none()
	}
}
