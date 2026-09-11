//! The agent's reqwest clients.

// spec:NETCHG

use std::{
	net::{IpAddr, SocketAddr},
	time::Duration,
};

// Reached only by the pieces the agent shares into a rebuilt client, each behind its own feature.
#[cfg(any(feature = "cookies", feature = "http3"))]
use std::sync::Arc;

use http::header::HeaderMap;

#[cfg(feature = "cache")]
mod http_cache;

#[cfg(feature = "cache")]
pub(crate) use http_cache::{HttpCacheRecipe, HttpCacheStore};

#[cfg(feature = "cache")]
use http_cache_reqwest::{Cache, HttpCache};
use reqwest::{Client, Identity, redirect::Policy, tls::Certificate};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
#[cfg(feature = "cookies")]
use web_faith_cookies::FaithJar;

#[cfg(feature = "dns")]
use web_faith_dns::FaithResolver;

#[cfg(feature = "dns")]
use crate::retry::StaleAddressRetry;

#[cfg(feature = "http3")]
use web_faith_alt_svc::{AltSvcCache, AltSvcMiddleware, H3Prober};

use crate::{
	error::{FaithError, FaithErrorKind},
	options::RedirectPolicy,
	retry::DeadConnectionRetry,
};

#[cfg(feature = "http3")]
use crate::timing::HeadersStamp;

// Chrome's shape: a 6 MiB stream inside a 15 MiB connection. A larger window measured faster, but
// a pooled server-side client multiplies per-connection memory across far more connections than a
// browser does (spec:FLOW).
pub(crate) const DEFAULT_STREAM_WINDOW: u32 = 6 * 1024 * 1024;
pub(crate) const DEFAULT_CONNECTION_WINDOW: u32 = 15 * 1024 * 1024;

// Concurrent streams share the connection's headroom, so the asymmetry is the point of the
// defaults rather than an accident of the numbers (spec:FLOW#common-windows).
const _: () = assert!(DEFAULT_CONNECTION_WINDOW > DEFAULT_STREAM_WINDOW);

/// The flow-control windows to apply, once the common group, the per-protocol overrides, and the
/// defaults have been reconciled.
// spec:FLOW#per-protocol-windows
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedWindows {
	pub(crate) stream: u32,
	pub(crate) connection: u32,
}

/// The Node.js networking environment variables, applied to a reqwest client builder as Node
/// honours them for its own. Read for every agent, so `fetch()` behaves like Node's built-in
/// fetch out of the box.
///
/// - `NODE_EXTRA_CA_CERTS`: a path to a PEM file whose certificates are added to
///   the trust store on top of the platform roots. As in Node.js, a value that
///   is empty, or points at a file that cannot be read or parsed, is ignored
///   rather than fatal, unlike an explicitly configured extra root, which is an
///   error. Certificates load in addition to any configured explicitly.
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
/// `NODE_USE_SYSTEM_CA` is not honoured: faith bundles no Mozilla
/// root set, so its only default trust source is the platform store the variable
/// would toggle. `=0` could therefore only mean "trust almost nothing", which is
/// never what a caller wants, so the platform store is always used.
///
/// Read once, at construction: these are layered on top of the explicit options when the agent is
/// built, so a client rebuilt later replays what was read then rather than picking up an environment
/// that has changed since.
// spec:NETCHG
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeEnvRecipe {
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

/// The HTTP/3 upgrade settings a client's middleware needs. The origin knowledge itself is not
/// here: it belongs to the agent and outlives any one client.
// spec:NETCHG
#[cfg(feature = "http3")]
#[derive(Debug, Clone)]
pub(crate) struct H3UpgradeRecipe {
	pub(crate) enabled: bool,
	pub(crate) attempt_timeout: Option<Duration>,
	pub(crate) probe: bool,
	pub(crate) probe_timeout: Option<Duration>,
}

/// Everything needed to build the agent's clients, validated once up front.
// spec:NETCHG
#[derive(Debug, Clone)]
pub(crate) struct ClientRecipe {
	pub(crate) user_agent: String,
	pub(crate) local_address: Option<IpAddr>,
	pub(crate) default_headers: Option<HeaderMap>,
	/// Under the system resolver no hickory resolver is installed at all. Only meaningful where
	/// Faith has a resolver of its own to choose instead.
	// spec:DNS
	#[cfg(feature = "dns")]
	pub(crate) dns_system: bool,
	/// Validated at construction, and applied whichever resolver is in use: reqwest layers
	/// overrides on top of the resolver it was given.
	// spec:DNS#overrides
	pub(crate) dns_overrides: Vec<(String, Vec<SocketAddr>)>,
	pub(crate) http2_adaptive_window: bool,
	/// `None` when adaptive windowing owns the windows itself.
	// spec:FLOW#adaptive-windowing
	pub(crate) http2_windows: Option<ResolvedWindows>,
	#[cfg(feature = "http3")]
	pub(crate) http3_max_idle_timeout: Duration,
	#[cfg(feature = "http3")]
	pub(crate) http3_windows: ResolvedWindows,
	#[cfg(feature = "http3")]
	pub(crate) http3_congestion_bbr: bool,
	#[cfg(feature = "http3")]
	pub(crate) http3_send_window: Option<u32>,
	pub(crate) pool_idle_timeout: Option<Duration>,
	pub(crate) pool_max_idle_per_host: Option<usize>,
	pub(crate) redirect: Option<RedirectPolicy>,
	pub(crate) connect_timeout: Option<Duration>,
	pub(crate) read_timeout: Option<Duration>,
	pub(crate) total_timeout: Option<Duration>,
	/// Only reachable over QUIC, so only applied when HTTP/3 is compiled in.
	#[cfg(feature = "http3")]
	pub(crate) tls_early_data: Option<bool>,
	pub(crate) tls_identity: Option<Identity>,
	pub(crate) tls_required: Option<bool>,
	pub(crate) tls_extra_roots: Vec<Certificate>,
	pub(crate) node_env: NodeEnvRecipe,
	#[cfg(feature = "cache")]
	pub(crate) http_cache: Option<HttpCacheRecipe>,
	#[cfg(feature = "http3")]
	pub(crate) h3_upgrade: H3UpgradeRecipe,
}

/// Point the resolver's `HTTPS` record reading at the upgrade layer, so a record advertising
/// `alpn="h3"` makes an origin probe-worthy before anything has connected to it.
///
/// A no-op under the system resolver or with HTTP/3 upgrade off. Re-called on a network change,
/// where the prober is rebuilt with the client it sends on.
// spec:DNS#https-records
#[cfg(all(feature = "http3", feature = "dns"))]
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

/// The clients and prober [`ClientRecipe::build`] produces.
pub(crate) struct BuiltClients {
	pub(crate) client: ClientWithMiddleware,
	pub(crate) raw_client: Client,
	#[cfg(feature = "http3")]
	pub(crate) prober: Option<Arc<H3Prober>>,
}

/// Install ring as the process's rustls crypto provider.
///
/// reqwest reads the process default when it builds a client and panics if there is none. Only
/// where ring is the chosen backend; with `tls-aws-lc-rs` also on, reqwest supplies aws-lc-rs
/// itself. Once-only and process-wide, so a provider the embedding program installed is left
/// alone.
#[cfg(all(feature = "tls-ring", not(feature = "tls-aws-lc-rs")))]
fn install_crypto_provider() {
	use std::sync::Once;

	static ONCE: Once = Once::new();
	ONCE.call_once(|| {
		let _ = rustls::crypto::ring::default_provider().install_default();
	});
}

impl ClientRecipe {
	/// The window an idle pooled connection lives in, which is also how long a warm-up counts as
	/// warm and how long a connection stays listed.
	// spec:POOL
	// spec:WARM
	// spec:OBS
	pub fn conn_timeout(&self) -> Duration {
		// reqwest's own default, mirrored because the pool timeout it applies is not readable.
		self.pool_idle_timeout.unwrap_or(Duration::from_secs(90))
	}

	/// Build a fresh client and raw client, around state the agent already holds.
	///
	/// Everything passed in survives a rebuild by being shared rather than rebuilt: the cookie
	/// jar, the resolver (and so its cache), and the HTTP/3 origin knowledge all belong to the
	/// agent rather than to any one client.
	// spec:NETCHG#what-the-signal-keeps
	pub fn build(
		&self,
		#[cfg(feature = "cookies")] cookie_jar: Option<&Arc<FaithJar>>,
		#[cfg(feature = "dns")] dns_resolver: Option<&FaithResolver>,
		#[cfg(feature = "http3")] alt_svc_cache: Option<&Arc<AltSvcCache>>,
	) -> Result<BuiltClients, FaithError> {
		#[cfg(all(feature = "tls-ring", not(feature = "tls-aws-lc-rs")))]
		install_crypto_provider();

		let mut client = Client::builder()
			.tls_info(true)
			.tls_sslkeylogfile(true)
			.user_agent(self.user_agent.clone());

		if let Some(ip) = self.local_address {
			client = client.local_address(ip);
		}

		#[cfg(feature = "cookies")]
		if let Some(jar) = cookie_jar {
			client = client.cookie_provider(jar.clone());
		}

		// Registered whichever resolver is in use: reqwest layers overrides on top of the
		// resolver it was given, so they take effect under the system resolver too
		// (spec:DNS#overrides).
		for (domain, addresses) in &self.dns_overrides {
			client = client.resolve_to_addrs(domain, addresses);
		}

		#[cfg(feature = "dns")]
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

		#[cfg(feature = "cache")]
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
		#[cfg(feature = "dns")]
		{
			client = client.with(StaleAddressRetry::new(dns_resolver.cloned()));
		}

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
