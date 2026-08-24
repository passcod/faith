//! The agent: what owns a connection pool, and the verbs that act on a live one.

// spec:AGENT spec:WARM spec:NETCHG spec:OBS

use std::{
	future::Future,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use http::header::HeaderValue;
use moka::sync::Cache as MokaCache;
use reqwest::{Client, Version};
use reqwest_middleware::ClientWithMiddleware;
use url::Url;
use web_faith_conn_tracker::{ConnectionSnapshot, ConnectionTracker};
use web_faith_cookies::FaithJar;
use web_faith_dns::{FaithResolver, ResolverReport};

#[cfg(feature = "http3")]
use web_faith_alt_svc::{AltSvcCache, H3Prober};

use crate::{
	client::ClientRecipe,
	error::{FaithError, FaithErrorKind},
	stats::{AgentStats, InnerAgentStats},
	warm_up::{extract_host, origin_key, reduce_to_origin},
};

#[cfg(feature = "http3")]
use crate::client::install_https_sink;

/// The agent settings a request consults, as opposed to those a client is built from.
#[derive(Debug, Clone, Default)]
pub struct AgentSettings {
	/// Whether an HTTP/3 upgrade may follow a port the origin advertised, which a request needs so
	/// a rewritten port is not reported as a redirect.
	pub h3_follow_advertised_port: bool,
	/// Whether the upgrade machinery is on at all. A warm-up needs it to route the way a foreground
	/// request would.
	#[cfg(feature = "http3")]
	pub h3_upgrade_enabled: bool,
	/// Whether a streaming request body may go out over HTTP/1.x.
	pub quirk_h1_request_streaming: bool,
	/// The agent's default `Accept-Encoding`, if one sits among its default headers, which decides
	/// which codings a response is decoded under when a request adds none of its own.
	pub default_accept_encoding: Option<HeaderValue>,
	/// The agent's default `Content-Encoding`, if one sits among its default headers, which a
	/// request layers its own coding on top of rather than displacing.
	pub default_content_encoding: Option<HeaderValue>,
	/// Whether a `Priority` header sits among the agent's default headers, so that default wins
	/// over the one a request's priority would derive.
	pub has_default_priority: bool,
}

/// An HTTP client with its own connection pool, caches, and resolver.
///
/// Cloning one is cheap and every clone names the same underlying agent, which is what lets a
/// request take a handle of its own without opening a second pool.
#[derive(Debug, Clone)]
pub struct Agent {
	/// `None` once [`Agent::close`] has been called. The heavy resources
	/// (connection pool, DNS resolver, background tasks) live inside this
	/// client, so dropping it is what actually releases them.
	pub client: Option<ClientWithMiddleware>,
	/// The raw `reqwest::Client` underlying [`Self::client`], sharing its connection pool. A
	/// `preconnect` warm-up sends its synthetic request here rather than through the middleware
	/// stack, which bypasses the HTTP cache and the Alt-Svc layer (and so keeps the warm-up out of
	/// request accounting), while still pooling the connection foreground requests reuse. `None`
	/// once the agent is closed. (spec:WARM)
	pub raw_client: Option<Client>,
	/// Faith's DNS resolver, shared with [`Self::client`] so `prefetchDns` warms the cache requests
	/// read. `None` under the system resolver, where there is no such cache. (spec:WARM)
	pub dns_resolver: Option<FaithResolver>,
	/// Origins with a warm-up connection opened within the pool idle window, so a repeat
	/// `preconnect` does no new work. Keyed by `scheme://host:port`; entries expire with the idle
	/// timeout. (spec:WARM)
	pub warmed: MokaCache<String, ()>,
	/// Single-flight claims for in-flight `preconnect` warm-ups, so concurrent calls for the same
	/// origin do not open duplicate connections. (spec:WARM)
	pub warming: MokaCache<String, ()>,
	/// Bumped by `networkChanged`, so a warm-up that was in flight across the signal does not
	/// record its origin as warm: its connection went into the pool that was just dropped
	/// (spec:NETCHG#reach-across-the-subsystems).
	pub warm_generation: Arc<AtomicU64>,
	pub cookie_jar: Option<Arc<FaithJar>>,
	pub stats: Arc<InnerAgentStats>,
	pub conn_tracker: Arc<ConnectionTracker>,
	#[cfg(feature = "http3")]
	#[allow(dead_code)]
	pub alt_svc_cache: Option<Arc<AltSvcCache>>,
	/// Held so `close()` can abort in-flight background probes: each one owns a
	/// clone of the raw client, which would otherwise keep the connection pool
	/// alive past close for up to the probe timeout.
	#[cfg(feature = "http3")]
	pub h3_prober: Option<Arc<H3Prober>>,
	/// Mirrors `http3.upgradeFollowAdvertisedPort`. Lives here because `fetch` needs
	/// it to stop a rewritten port from being reported as a redirect.
	pub h3_follow_advertised_port: bool,
	/// Mirrors `http3.upgradeEnabled`. A warm-up needs it to route the way a foreground request
	/// would: with the upgrade machinery off, nothing upgrades, whatever the caches hold.
	/// (spec:WARM#preconnect)
	#[cfg(feature = "http3")]
	pub h3_upgrade_enabled: bool,
	/// Mirrors `quirks.h1RequestStreaming`. `fetch` consults it to decide whether a streaming
	/// request body may go out over HTTP/1.x (spec:QUIRK#http-1-x-request-body-streaming).
	pub quirk_h1_request_streaming: bool,
	/// The agent's default `Accept-Encoding`, if one was set among its default headers.
	/// `fetch` consults it to decide which codings to decode when a request adds none of
	/// its own (see [`web_faith_encoding`]).
	pub default_accept_encoding: Option<HeaderValue>,
	/// The agent's default `Content-Encoding`, if one was set among its default headers.
	/// `fetch` consults it when the `compress` option layers a coding on top of what a
	/// request already declares, since setting the joined value on the request would
	/// otherwise displace this default rather than build on it (spec:ENC).
	pub default_content_encoding: Option<HeaderValue>,
	/// Whether a `Priority` header sits among the agent's default headers. `fetch` consults
	/// it so that default wins over the header the `priority` option would derive.
	pub has_default_priority: bool,
	/// How to build this agent's clients, so `networkChanged` can build them again
	/// (spec:NETCHG). Shared rather than cloned per agent clone: every clone builds the same
	/// client from the same recipe, and `fetch` clones the agent per request.
	pub recipe: Arc<ClientRecipe>,
}

impl Agent {
	/// Build an agent from a validated recipe.
	///
	/// The recipe is what a client is built from, and the settings are what each request consults;
	/// validating whatever a caller expressed them as belongs to the surface that took it.
	pub fn build(
		recipe: ClientRecipe,
		settings: AgentSettings,
		cookie_jar: Option<Arc<FaithJar>>,
		dns_resolver: Option<FaithResolver>,
		#[cfg(feature = "http3")] alt_svc_cache: Option<Arc<AltSvcCache>>,
	) -> Result<Self, FaithError> {
		let conn_timeout = recipe.conn_timeout();
		let built = recipe.build(
			cookie_jar.as_ref(),
			dns_resolver.as_ref(),
			#[cfg(feature = "http3")]
			alt_svc_cache.as_ref(),
		)?;

		// Only now do all three exist: the resolver is built before the cache, and the prober
		// holds a client that holds the resolver, so this is the earliest the loop can be closed
		// (spec:DNS#https-records).
		#[cfg(feature = "http3")]
		install_https_sink(
			dns_resolver.as_ref(),
			alt_svc_cache.as_ref(),
			built.prober.as_ref(),
			recipe.h3_upgrade.enabled,
		);

		Ok(Self {
			client: Some(built.client),
			raw_client: Some(built.raw_client),
			dns_resolver,
			// A warm-up connection is warm only as long as the pool keeps it idle, so the record
			// that an origin is warm expires with that same window.
			warmed: MokaCache::builder().time_to_live(conn_timeout).build(),
			// A safety TTL well past any reasonable warm-up, so a claim that never gets released
			// (a warm-up whose task is dropped) frees the origin rather than wedging it.
			warming: MokaCache::builder()
				.time_to_live(Duration::from_secs(300))
				.build(),
			warm_generation: Default::default(),
			cookie_jar,
			stats: Default::default(),
			conn_tracker: ConnectionTracker::new(conn_timeout),
			#[cfg(feature = "http3")]
			alt_svc_cache,
			#[cfg(feature = "http3")]
			h3_prober: built.prober,
			h3_follow_advertised_port: settings.h3_follow_advertised_port,
			#[cfg(feature = "http3")]
			h3_upgrade_enabled: recipe.h3_upgrade.enabled,
			quirk_h1_request_streaming: settings.quirk_h1_request_streaming,
			default_accept_encoding: settings.default_accept_encoding,
			default_content_encoding: settings.default_content_encoding,
			has_default_priority: settings.has_default_priority,
			recipe: Arc::new(recipe),
		})
	}

	/// Close the agent, releasing its connection pool, DNS resolver, and any
	/// background tasks it owns, rather than waiting for the garbage collector
	/// to drop it. This is worth doing when you create many short-lived agents;
	/// a single long-lived agent can just be left to the GC.
	///
	/// Requests already in flight run to completion. Any new request on a closed
	/// agent throws a `Closed` error. Calling `close()` more than once is a
	/// no-op. The cookie store, if any, remains readable via `getCookie`.
	pub fn close(&mut self) {
		// Dropping the client releases the reqwest connection pool and the
		// Hickory resolver task; the alt-svc cache goes with it. The raw client
		// shares that pool and the resolver, so it goes too, and both are what a
		// later `preconnect`/`prefetchDns` checks to throw the closed-agent error.
		self.client = None;
		self.raw_client = None;
		self.dns_resolver = None;
		#[cfg(feature = "http3")]
		{
			// Probes hold a raw client clone; abort them so the pool doesn't
			// outlive close by up to the probe timeout.
			if let Some(prober) = &self.h3_prober {
				prober.abort_all();
			}
			self.h3_prober = None;
			self.alt_svc_cache = None;
		}
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
	///
	/// spec:NETCHG
	pub fn network_changed(&mut self) {
		// A closed agent has already released all of this.
		if self.client.is_none() {
			return;
		}

		// reqwest cannot drop pooled connections short of dropping the client, so the client is
		// rebuilt from the recipe the agent kept for this. Requests in flight hold their own
		// clone of the old client (`fetch` clones the agent per request), so they run to
		// completion and the old pool goes when the last of them finishes.
		//
		// A rebuild that fails leaves the agent on its existing client: the options were already
		// validated at construction, so a failure here is not the caller's to answer for, and an
		// agent that still works on the old network beats one that works nowhere.
		let built = self.recipe.build(
			self.cookie_jar.as_ref(),
			self.dns_resolver.as_ref(),
			#[cfg(feature = "http3")]
			self.alt_svc_cache.as_ref(),
		);
		if let Ok(built) = built {
			#[cfg(feature = "http3")]
			{
				// Abort probes running on the old client: each holds a clone of it, and their
				// answers would describe the path that has just gone away.
				if let Some(prober) = &self.h3_prober {
					prober.abort_all();
				}
				self.h3_prober = built.prober;
				// The sink holds the prober, which has just been replaced along with the client
				// it sends on; leaving the old one installed would aim DNS-triggered probes at a
				// client that has been dropped.
				install_https_sink(
					self.dns_resolver.as_ref(),
					self.alt_svc_cache.as_ref(),
					self.h3_prober.as_ref(),
					self.h3_upgrade_enabled,
				);
			}
			self.client = Some(built.client);
			self.raw_client = Some(built.raw_client);
		}

		// Names resolve afresh against the new network, through that network's own servers: the
		// resolver drops what it read off the old one and reads again when next used. Under the
		// system resolver there is no resolver here and so nothing to reset (spec:DNS).
		if let Some(resolver) = &self.dns_resolver {
			resolver.reset();
		}

		#[cfg(feature = "http3")]
		if let Some(alt_svc_cache) = &self.alt_svc_cache {
			alt_svc_cache.network_changed();
		}

		// The warm-up records describe pooled connections that have just been dropped, so a
		// `preconnect` after the signal opens a connection rather than finding the origin warm
		// (spec:NETCHG, spec:WARM). The single-flight claims are left alone: a warm-up still in
		// flight is not duplicated by releasing its claim, and the generation bump is what stops
		// it recording an origin as warm on the strength of a connection in the dropped pool.
		self.warmed.invalidate_all();
		self.warm_generation.fetch_add(1, Ordering::Relaxed);
	}

	/// Add a cookie into the agent.
	///
	/// The cookie goes through the same rules a `Set-Cookie` header would, with the url supplying
	/// the scheme and host they read, so this does nothing if:
	/// - the cookie store is disabled
	/// 	/// - the cookie does not parse
	/// - a `__Host-` or `__Secure-` name prefix is not satisfied
	/// - the cookie is larger than `cookies.maxSize`
	pub fn add_cookie(&self, url: &Url, cookie: &str) {
		let Some(jar) = &self.cookie_jar else {
			return;
		};

		jar.add_cookie_str(cookie, url);
	}

	/// Retrieve a cookie from the store.
	///
	/// `None` if:
	/// - there's no cookie at this url
	/// - the cookie store is disabled
	/// 	/// - the cookie cannot be represented as a string
	pub fn cookie_header(&self, url: &Url) -> Option<String> {
		let Some(jar) = &self.cookie_jar else {
			return None;
		};

		jar.request_cookie_header(url)
			.and_then(|val| val.to_str().ok().map(ToOwned::to_owned))
	}

	/// Returns statistics gathered by this agent:
	///
	/// - `requestsSent`
	/// - `responsesReceived`
	/// - `bodiesStarted`
	/// - `bodiesFinished`
	pub fn stats(&self) -> AgentStats {
		self.stats.snapshot()
	}

	/// Returns information on current connections open by this agent.
	///
	/// Only tracks TCP connections currently (upstream limitation). Stats are updated once a second:
	/// this makes it possible to track indicators over time to find the retransmission rate, for
	/// example. The `lostPackets` and `deliveryRateBps` stats are only available on Linux. Some other
	/// fields might also be missing depending on platform support; and no forward guarantees are made
	/// on field availability. If the platform isn't supported at all, this will always return empty.
	pub fn connections(&self) -> Vec<ConnectionSnapshot> {
		self.conn_tracker.snapshot()
	}

	/// Returns the DNS servers this agent resolves through, in the order they are queried, so
	/// "are my lookups actually encrypted" is answerable from inside the process.
	///
	/// Each entry gives the server's address, the transport in use (`udp`, `tcp`, `tls`, `https`,
	/// `quic`, or `h3`), and how that transport was arrived at (`configured` or `conventional`).
	/// The list is empty until the resolver has been used, because it reads its configuration on
	/// first use, and empty for an agent using the system resolver. (spec:OBS#resolvers)
	pub fn resolvers(&self) -> Vec<ResolverReport> {
		self.dns_resolver
			.as_ref()
			.map(FaithResolver::resolvers)
			.unwrap_or_default()
	}

	/// Note that a request reached this origin, so it holds a connection the pool keeps idle for
	/// the idle window and a `preconnect` for it has no new work to do (spec:WARM).
	///
	/// Called for foreground requests as well as warm-ups, because the criterion is about the
	/// origin holding an idle pooled connection, not about how it came to hold one.
	pub fn mark_warm(&self, url: &Url) {
		self.warmed.insert(origin_key(url), ());
	}

	/// Whether [`Self::close`] has been called.
	pub fn is_closed(&self) -> bool {
		self.client.is_none()
	}

	/// Warm the DNS cache for `host`, so a later request to it skips the lookup.
	///
	/// The argument is a bare host; a scheme, port, or path in a fuller string is ignored. The
	/// returned future completes when the answer lands in the cache and never fails, whatever
	/// happens on the network: the work is advisory. Under the system resolver there is no cache to
	/// warm, so it completes having done nothing. A host with nothing to resolve, or a closed agent,
	/// is refused here rather than by the future.
	// spec:WARM
	pub fn prefetch_dns(&self, host: &str) -> Result<impl Future<Output = ()> + use<>, FaithError> {
		if self.is_closed() {
			return Err(FaithErrorKind::Closed.into());
		}

		let Some(host) = extract_host(host) else {
			return Err(FaithErrorKind::AddressParse.into());
		};

		let resolver = self.dns_resolver.clone();
		Ok(async move {
			if let Some(resolver) = resolver {
				resolver.prefetch(&host).await;
			}
		})
	}

	/// Open a pooled connection to `origin`, so the first request to it skips DNS, TCP, and TLS
	/// setup.
	///
	/// The argument is an origin (`scheme://host[:port]`); a longer URL is reduced to one. The
	/// warm-up sends a synthetic `HEAD` to the origin's root -- the origin sees it -- over the
	/// transport the next foreground request would use: a confirmed HTTP/3 origin gets a warm QUIC
	/// connection, every other origin a TCP one. The returned future completes when the attempt
	/// finishes and never fails: every network failure is quiet. Something that cannot be connected
	/// to, or a closed agent, is refused here rather than by the future.
	// spec:WARM
	pub fn preconnect(&self, origin: &str) -> Result<impl Future<Output = ()> + use<>, FaithError> {
		let Some(raw_client) = self.raw_client.clone() else {
			return Err(FaithErrorKind::Closed.into());
		};

		let Some(url) = reduce_to_origin(origin) else {
			return Err(FaithErrorKind::AddressParse.into());
		};
		let key = origin_key(&url);

		// Already warm within the idle window, or a warm-up for this origin already in flight:
		// either way there is no new work to do, so finish without opening a duplicate.
		let redundant = self.warmed.contains_key(&key)
			|| !self.warming.entry(key.clone()).or_insert(()).is_fresh();

		// The transport the next foreground request would take, decided exactly as the Alt-Svc
		// layer decides it: nothing upgrades with the machinery off; with a prober, only a
		// confirmed origin routes to QUIC (an advertisement is evidence worth probing, not worth
		// routing on); without one, the inline upgrade acts on advertisements too. Diverging here
		// would warm the wrong transport.
		// spec:WARM#preconnect
		#[cfg(feature = "http3")]
		let h3_port = self
			.alt_svc_cache
			.as_ref()
			.filter(|_| self.h3_upgrade_enabled)
			.and_then(|cache| {
				if self.h3_prober.is_some() {
					cache.confirmed_port(&url)
				} else {
					cache.should_use_h3(&url)
				}
			});
		#[cfg(not(feature = "http3"))]
		let h3_port: Option<u16> = None;

		let conn_tracker = self.conn_tracker.clone();
		let warmed = self.warmed.clone();
		let warming = self.warming.clone();
		// Read before the warm-up starts, to compare against once it finishes.
		let warm_generation = self.warm_generation.clone();
		let generation = warm_generation.load(Ordering::Relaxed);

		Ok(async move {
			if redundant {
				return;
			}

			// Release the single-flight claim whatever happens, so a later warm-up is not blocked
			// by this one having finished.
			struct ReleaseClaim {
				warming: MokaCache<String, ()>,
				key: String,
			}
			impl Drop for ReleaseClaim {
				fn drop(&mut self) {
					self.warming.invalidate(&self.key);
				}
			}
			let _release = ReleaseClaim {
				warming,
				key: key.clone(),
			};

			let request = match h3_port {
				Some(port) => {
					let mut h3_url = url.clone();
					// A port differing from the origin's only comes back with the
					// follow-advertised-port option on; rewriting the URL is how reqwest is told to
					// connect there, mirroring the foreground path.
					if Some(port) != h3_url.port_or_known_default() {
						let _ = h3_url.set_port(Some(port));
					}
					raw_client.head(h3_url).version(Version::HTTP_3)
				}
				None => raw_client.head(url.clone()),
			};

			let outcome = request.send().await;

			// A TCP warm-up leaves a pooled connection to track; a QUIC one does not (QUIC
			// connections are not tracked, and a confirmed origin has nothing left to probe).
			if h3_port.is_none()
				&& let Ok(response) = &outcome
				&& let Some(info) = response
					.extensions()
					.get::<hyper_util::client::legacy::connect::HttpInfo>()
			{
				conn_tracker.track_warmup(info.local_addr(), info.remote_addr());
			}

			// A network change while this was in flight leaves the origin unmarked: the connection
			// landed in the pool that change dropped, so it is not warm however well the request
			// went.
			// spec:NETCHG#reach-across-the-subsystems
			if outcome.is_ok() && warm_generation.load(Ordering::Relaxed) == generation {
				warmed.insert(key, ());
			}
		})
	}
}
