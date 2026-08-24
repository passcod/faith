//! Building the agent's reqwest clients from validated options.
//!
//! The recipe here is what lets a client be rebuilt: `network_changed` has to drop the connection
//! pool, and reqwest offers no way to do that short of dropping the client, so building one is a
//! pure function of settings that were validated once (spec:NETCHG).

use std::{
	net::{IpAddr, SocketAddr},
	sync::Arc,
	time::Duration,
};

use http::header::HeaderMap;
use http_cache_reqwest::{
	CACacheManager, Cache, CacheMode, HttpCache, HttpCacheOptions, MokaManager,
};
use reqwest::{Client, Identity, redirect::Policy, tls::Certificate};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use web_faith_cookies::FaithJar;
use web_faith_dns::FaithResolver;

#[cfg(feature = "http3")]
use web_faith_alt_svc::{AltSvcCache, AltSvcMiddleware, H3Prober};

use crate::{
	error::{FaithError, FaithErrorKind},
	retry::{DeadConnectionRetry, StaleAddressRetry},
};

#[cfg(feature = "http3")]
use crate::timing::HeadersStamp;

/// What to do with a redirect response.
///
/// The Node surface spells these as fetch's own `redirect` values; this is the same choice in the
/// client's own terms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RedirectPolicy {
	/// Follow redirects, up to the standard's limit.
	#[default]
	Follow,
	/// Refuse a redirect, reporting it as an error.
	Error,
	/// Return the redirect response itself rather than following it.
	Stop,
}

/// Per-stream receive window applied to both protocols when nothing overrides it (spec:FLOW).
///
/// Chrome's shape: 6 MiB stream inside a 15 MiB connection. Picked over a larger window that
/// measured faster because it is what browsers have proven at scale, and because a pooled
/// server-side client multiplies per-connection memory across far more connections.
pub const DEFAULT_STREAM_WINDOW: u32 = 6 * 1024 * 1024;

/// Whole-connection receive window applied to both protocols when nothing overrides it (spec:FLOW).
pub const DEFAULT_CONNECTION_WINDOW: u32 = 15 * 1024 * 1024;

// Concurrent streams share the connection's headroom, so the asymmetry is the point of the
// defaults rather than an accident of the numbers (spec:FLOW#common-windows).
const _: () = assert!(DEFAULT_CONNECTION_WINDOW > DEFAULT_STREAM_WINDOW);

/// The flow-control windows to apply, once the common group, the per-protocol overrides, and the
/// defaults have been reconciled (spec:FLOW#per-protocol-windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedWindows {
	pub stream: u32,
	pub connection: u32,
}

/// What the Node.js networking environment variables asked for, to apply to a reqwest client
/// builder as Node.js honours them for its own clients. This is read for every agent, so
/// `fetch()` behaves like Node's built-in fetch out of the box.
///
/// - `NODE_EXTRA_CA_CERTS`: a path to a PEM file whose certificates are added to
///   the trust store on top of the platform roots. As in Node.js, a value that
///   is empty, or points at a file that cannot be read or parsed, is ignored
///   rather than fatal — unlike the explicit [`AgentTlsOptions::extra_roots`]
///   option, which throws. Certificates load in addition to any `extra_roots`.
///
/// - `NODE_TLS_REJECT_UNAUTHORIZED`: when set to exactly `"0"`, TLS certificate
///   validation is disabled for the agent. This is insecure and exists only to
///   match Node.js semantics; any other value leaves validation enabled.
///
/// - `NODE_USE_ENV_PROXY`: when set to exactly `"0"`, the agent ignores the
///   ambient proxy configuration (`HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` and the
///   OS proxy settings) that reqwest reads by default. Unlike Node.js — where
///   env-proxy support is opt-*in* and off by default — faith reads it by
///   default and treats this variable purely as an opt-*out* switch, so leaving
///   it unset (or `"1"`) keeps the existing always-on behaviour.
///
/// `NODE_USE_SYSTEM_CA` is deliberately not honoured: faith bundles no Mozilla
/// root set, so its only default trust source is the platform store the variable
/// would toggle. `=0` could therefore only mean "trust almost nothing", which is
/// never what a caller wants, so the platform store is always used.
///
/// Read once, at construction: AGENT has these layered on top of the explicit options when the
/// agent is built, so a client rebuilt later (spec:NETCHG) replays what was read then rather than
/// picking up an environment that has changed since.
#[derive(Debug, Clone, Default)]
pub struct NodeEnvRecipe {
	extra_ca_certs: Vec<Certificate>,
	accept_invalid_certs: bool,
	no_proxy: bool,
}

impl NodeEnvRecipe {
	pub fn read() -> Self {
		let mut recipe = Self::default();

		if let Ok(path) = std::env::var("NODE_EXTRA_CA_CERTS")
			&& !path.is_empty()
			&& let Ok(bytes) = std::fs::read(&path)
			&& let Ok(certs) = Certificate::from_pem_bundle(&bytes)
		{
			recipe.extra_ca_certs = certs;
		}

		recipe.accept_invalid_certs =
			std::env::var("NODE_TLS_REJECT_UNAUTHORIZED").as_deref() == Ok("0");
		recipe.no_proxy = std::env::var("NODE_USE_ENV_PROXY").as_deref() == Ok("0");

		recipe
	}

	fn apply(&self, mut client: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
		if !self.extra_ca_certs.is_empty() {
			client = client.tls_certs_merge(self.extra_ca_certs.iter().cloned());
		}

		if self.accept_invalid_certs {
			client = client.danger_accept_invalid_certs(true);
		}

		if self.no_proxy {
			client = client.no_proxy();
		}

		client
	}
}

/// The HTTP cache store to install on a client, held as the built manager rather than as the
/// options that produced it.
///
/// The manager *is* the store: `MokaManager` holds the cached entries behind an `Arc`, and
/// `CACacheManager` names the directory holding them. So cloning one shares the cache, while
/// building a fresh one from the same options would empty an in-memory cache — which is why a
/// client rebuilt for a network change clones this (spec:NETCHG#what-the-signal-keeps).
#[derive(Debug, Clone)]
pub enum HttpCacheStore {
	Disk(CACacheManager),
	Memory(MokaManager),
}

/// The HTTP cache middleware to install.
#[derive(Debug, Clone)]
pub struct HttpCacheRecipe {
	pub mode: CacheMode,
	pub options: HttpCacheOptions,
	pub store: HttpCacheStore,
}

/// The HTTP/3 upgrade settings a client's middleware needs. The origin knowledge itself is not
/// here: it belongs to the agent and outlives any one client (spec:NETCHG).
#[cfg(feature = "http3")]
#[derive(Debug, Clone)]
pub struct H3UpgradeRecipe {
	pub enabled: bool,
	pub attempt_timeout: Option<Duration>,
	pub probe: bool,
	pub probe_timeout: Option<Duration>,
}

/// Everything needed to build the agent's clients, validated once at construction.
///
/// This exists because `networkChanged` has to drop the connection pool, and reqwest offers no way
/// to do that short of dropping the client, so the client has to be buildable more than once
/// (spec:NETCHG). `AgentOptions` cannot serve: validating it consumes it, and it carries napi
/// values belonging to the JS call that passed them. So validation happens once, into these
/// Rust-native fields, and building a client is a pure function of them and the agent's shared
/// state.
#[derive(Debug, Clone)]
pub struct ClientRecipe {
	pub user_agent: String,
	pub local_address: Option<IpAddr>,
	pub default_headers: Option<HeaderMap>,
	/// Under the system resolver no hickory resolver is installed at all (spec:DNS).
	pub dns_system: bool,
	/// Validated at construction, and applied whichever resolver is in use: reqwest layers
	/// overrides on top of the resolver it was given (spec:DNS#overrides).
	pub dns_overrides: Vec<(String, Vec<SocketAddr>)>,
	pub http2_adaptive_window: bool,
	/// `None` when adaptive windowing owns the windows itself (spec:FLOW#adaptive-windowing).
	pub http2_windows: Option<ResolvedWindows>,
	#[cfg(feature = "http3")]
	pub http3_max_idle_timeout: Duration,
	#[cfg(feature = "http3")]
	pub http3_windows: ResolvedWindows,
	#[cfg(feature = "http3")]
	pub http3_congestion_bbr: bool,
	#[cfg(feature = "http3")]
	pub http3_send_window: Option<u32>,
	pub pool_idle_timeout: Option<Duration>,
	pub pool_max_idle_per_host: Option<usize>,
	pub redirect: Option<RedirectPolicy>,
	pub connect_timeout: Option<Duration>,
	pub read_timeout: Option<Duration>,
	pub total_timeout: Option<Duration>,
	/// Only reachable over QUIC, so only applied when HTTP/3 is compiled in.
	#[cfg(feature = "http3")]
	pub tls_early_data: Option<bool>,
	pub tls_identity: Option<Identity>,
	pub tls_required: Option<bool>,
	pub tls_extra_roots: Vec<Certificate>,
	pub node_env: NodeEnvRecipe,
	pub http_cache: Option<HttpCacheRecipe>,
	#[cfg(feature = "http3")]
	pub h3_upgrade: H3UpgradeRecipe,
}

/// Point the resolver's `HTTPS` record reading at the upgrade layer, so a record advertising
/// `alpn="h3"` makes an origin probe-worthy before anything has connected to it.
///
/// A no-op without all the parts: the system resolver is not Faith's to add a query to, and with
/// HTTP/3 upgrade off there is nothing an advertisement could feed, so neither sends one
/// (spec:DNS#https-records).
///
/// Re-called on a network change, where the prober is rebuilt with the client it sends on.
#[cfg(feature = "http3")]
pub fn install_https_sink(
	dns_resolver: Option<&FaithResolver>,
	alt_svc_cache: Option<&Arc<AltSvcCache>>,
	prober: Option<&Arc<H3Prober>>,
	upgrade_enabled: bool,
) {
	if !upgrade_enabled {
		return;
	}
	let (Some(resolver), Some(cache)) = (dns_resolver, alt_svc_cache) else {
		return;
	};
	resolver.set_https_sink(Arc::new(web_faith_alt_svc::H3HttpsSink::new(
		Arc::clone(cache),
		prober,
	)));
}

/// The clients [`ClientRecipe::build`] produces, and the prober that sends on them.
pub struct BuiltClients {
	pub client: ClientWithMiddleware,
	pub raw_client: Client,
	#[cfg(feature = "http3")]
	pub prober: Option<Arc<H3Prober>>,
}

impl ClientRecipe {
	/// The window an idle pooled connection lives in, which is also how long a warm-up counts as
	/// warm and how long a connection stays listed (spec:POOL, spec:WARM, spec:OBS).
	pub fn conn_timeout(&self) -> Duration {
		// reqwest's own default, mirrored because the pool timeout it applies is not readable.
		self.pool_idle_timeout.unwrap_or(Duration::from_secs(90))
	}

	/// Build a fresh client and raw client, around state the agent already holds.
	///
	/// Everything passed in survives a rebuild by being shared rather than rebuilt: the cookie
	/// jar, the resolver (and so its cache), and the HTTP/3 origin knowledge all belong to the
	/// agent rather than to any one client (spec:NETCHG#what-the-signal-keeps).
	pub fn build(
		&self,
		cookie_jar: Option<&Arc<FaithJar>>,
		dns_resolver: Option<&FaithResolver>,
		#[cfg(feature = "http3")] alt_svc_cache: Option<&Arc<AltSvcCache>>,
	) -> Result<BuiltClients, FaithError> {
		let mut client = Client::builder()
			.tls_info(true)
			.tls_sslkeylogfile(true)
			.user_agent(self.user_agent.clone());

		if let Some(ip) = self.local_address {
			client = client.local_address(ip);
		}

		if let Some(jar) = cookie_jar {
			client = client.cookie_provider(jar.clone());
		}

		// Registered whichever resolver is in use: reqwest layers overrides on top of the
		// resolver it was given, so they take effect under the system resolver too
		// (spec:DNS#overrides).
		for (domain, addresses) in &self.dns_overrides {
			client = client.resolve_to_addrs(domain, addresses);
		}

		if self.dns_system {
			client = client.no_hickory_dns();
		} else if let Some(resolver) = dns_resolver {
			client = client.dns_resolver(resolver.clone());
		}

		if let Some(headers) = &self.default_headers {
			client = client.default_headers(headers.clone());
		}

		if self.http2_adaptive_window {
			client = client.http2_adaptive_window(true);
		} else if let Some(windows) = self.http2_windows {
			client = client
				.http2_initial_stream_window_size(windows.stream)
				.http2_initial_connection_window_size(windows.connection);
		}

		#[cfg(feature = "http3")]
		{
			client = client
				.http3_max_idle_timeout(self.http3_max_idle_timeout)
				.http3_stream_receive_window(self.http3_windows.stream.into())
				.http3_conn_receive_window(self.http3_windows.connection.into());

			if self.http3_congestion_bbr {
				client = client.http3_congestion_bbr();
			}

			if let Some(send_window) = self.http3_send_window {
				client = client.http3_send_window(send_window.into());
			}
		}

		if let Some(timeout) = self.pool_idle_timeout {
			client = client.pool_idle_timeout(Some(timeout));
		}

		if let Some(max_idle) = self.pool_max_idle_per_host {
			client = client.pool_max_idle_per_host(max_idle);
		}

		match self.redirect {
			// follow is the default, and we ignore manual
			None | Some(RedirectPolicy::Follow) => {}
			Some(RedirectPolicy::Error) => {
				client = client.redirect(Policy::custom(|attempt| {
					// Hand reqwest the error unboxed: it boxes for us, and boxing first would
					// put a `Box<FaithError>` in the source chain, which does not downcast
					// back to `FaithError` when we come to recover the kind as a `code`.
					attempt.error(FaithError::from(FaithErrorKind::Redirect))
				}));
			}
			Some(RedirectPolicy::Stop) => {
				client = client.redirect(Policy::none());
			}
		}

		if let Some(timeout) = self.connect_timeout {
			client = client.connect_timeout(timeout);
		}

		if let Some(timeout) = self.read_timeout {
			client = client.read_timeout(timeout);
		}

		if let Some(timeout) = self.total_timeout {
			client = client.timeout(timeout);
		}

		#[cfg(feature = "http3")]
		if let Some(early_data) = self.tls_early_data {
			client = client.tls_early_data(early_data);
		}

		if let Some(identity) = &self.tls_identity {
			client = client.identity(identity.clone());
		}

		if let Some(https_only) = self.tls_required {
			client = client.https_only(https_only);
		}

		if !self.tls_extra_roots.is_empty() {
			client = client.tls_certs_merge(self.tls_extra_roots.iter().cloned());
		}

		client = self.node_env.apply(client);

		let raw_client = client
			.build()
			.map_err(|e| FaithError::new(FaithErrorKind::Config, Some(format!("{e:?}"))))?;
		let mut client = ClientBuilder::new(raw_client.clone());

		#[cfg(feature = "http3")]
		let prober = {
			// The prober sends on the *raw* client, deliberately: it must skip
			// the HTTP cache (a replayed cached response would fake a
			// confirmation) and the Alt-Svc middleware (no recursion), while
			// sharing the h3 connection pool so a successful probe leaves a warm
			// connection for the foreground. Only built when both the upgrade
			// machinery and probing are on.
			alt_svc_cache
				.filter(|_| self.h3_upgrade.enabled && self.h3_upgrade.probe)
				.map(|cache| {
					Arc::new(H3Prober::new(
						raw_client.clone(),
						cache.clone(),
						self.h3_upgrade.probe_timeout,
					))
				})
		};

		if let Some(cache) = &self.http_cache {
			// The two arms differ only in the manager's type, which `HttpCache` is generic over,
			// so they cannot share a constructor without boxing the manager.
			client = match &cache.store {
				HttpCacheStore::Disk(manager) => client.with(Cache(HttpCache {
					mode: cache.mode,
					manager: manager.clone(),
					options: cache.options.clone(),
				})),
				HttpCacheStore::Memory(manager) => client.with(Cache(HttpCache {
					mode: cache.mode,
					manager: manager.clone(),
					options: cache.options.clone(),
				})),
			};
		}

		// Registered *after* the HTTP cache, so the Alt-Svc layer sits inside it:
		// `reqwest-middleware` runs the first-registered middleware outermost. Being
		// inside matters three times over.
		//
		// A cache hit is served without calling inward, so it never reaches this
		// layer. From outside, it would: `http-cache` rebuilds a cached response with
		// the *stored* HTTP version, so a response cached from an HTTP/3 exchange
		// replays as HTTP/3 and would be taken for a live one — confirming HTTP/3,
		// clearing cancellation strikes and refreshing the confirmed TTL on evidence
		// that never touched the network.
		//
		// The cache middleware also buffers the whole response body inside its own
		// call inward. From outside, the HTTP/3 attempt guarded here would span that
		// buffering, so a cancellation during body download would count as a strike,
		// and `upgradeAttemptTimeout` would bound body transfer rather than the wait
		// for response headers.
		//
		// And cache keys are computed before this layer runs, so an advertised-port
		// rewrite cannot split HTTP/3 and TCP responses across separate entries.
		#[cfg(feature = "http3")]
		if let Some(alt_svc_cache) = alt_svc_cache {
			client = client.with(AltSvcMiddleware::<HeadersStamp>::new(
				alt_svc_cache.clone(),
				self.h3_upgrade.enabled,
				self.h3_upgrade.attempt_timeout,
				prober.clone(),
			));
		}

		// Outside the dead-connection layer, so a re-resolved attempt gets the same
		// treatment as the original one: the two answer different questions, and a
		// fresh address deserves its own chance to draw a dead pooled connection.
		// Inside the Alt-Svc and cache layers for the reason given below.
		client = client.with(StaleAddressRetry::new(dns_resolver.cloned()));

		// Registered last, so it sits innermost and wraps nothing but the exchange
		// itself. Inside the Alt-Svc layer rather than outside it, because each
		// protocol attempt is its own connection and deserves its own retry: a
		// failed HTTP/3 attempt is the fallback's business, and re-running the
		// upgrade decision from out here would re-attempt HTTP/3 on a path already
		// judged dead and record a second failure against the origin for it. Inside
		// the HTTP cache for the same reason as the Alt-Svc layer -- a retry should
		// re-send the request, not redo the cache lookup that led to it.
		client = client.with(DeadConnectionRetry);

		Ok(BuiltClients {
			client: client.build(),
			raw_client,
			#[cfg(feature = "http3")]
			prober,
		})
	}
}
