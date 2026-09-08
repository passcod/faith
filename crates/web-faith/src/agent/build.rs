//! Turning options into an agent: validating what a caller expressed, and building from it.

// spec:AGENT

use std::{
	net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
	str::FromStr,
	sync::{Arc, RwLock},
	time::Duration,
};

use http::header::{HeaderMap, HeaderName, HeaderValue};

#[cfg(feature = "cache")]
use crate::{
	client::{HttpCacheRecipe, HttpCacheStore},
	options::CacheStore,
};

#[cfg(feature = "cache")]
use http_cache_reqwest::{
	CACacheManager, CacheOptions, HttpCacheOptions, MokaCacheBuilder, MokaManager,
};
use moka::sync::Cache as MokaCache;
use reqwest::{Identity, tls::Certificate};

#[cfg(feature = "connection-tracking")]
use web_faith_conn_tracker::ConnectionTracker;

#[cfg(feature = "cookies")]
use web_faith_cookies::FaithJar;

#[cfg(feature = "dns")]
use web_faith_dns::{
	DEFAULT_MAX_STALE, FaithResolver, ResolverSettings, ServerSpec, parse_domains,
};

#[cfg(feature = "http3")]
use web_faith_alt_svc::{AltSvcCache, AltSvcCacheConfig};

use crate::{
	USER_AGENT,
	agent::{Agent, AgentSettings, Live},
	client::{ClientRecipe, NodeEnvRecipe},
	error::{FaithError, FaithErrorKind},
	options::{AgentOptions, DnsOverride, Header, ipv6_wildcard_bindable, resolve_windows},
	request::PRIORITY,
};

#[cfg(feature = "http3")]
use crate::{client::H3UpgradeRecipe, options::Http3Congestion};

#[cfg(all(feature = "http3", feature = "dns"))]
use crate::client::install_https_sink;

impl Agent {
	/// This is what both surfaces land on, so the defaults a caller gets are settled here rather
	/// than once per surface.
	// spec:AGENT spec:NETCHG
	pub fn from_options(options: AgentOptions) -> Result<Self, FaithError> {
		// Destructured rather than read field by field so that a new option cannot be added
		// without the compiler pointing here, where every option is turned into the recipe the
		// agent's clients are built from (spec:NETCHG).
		let AgentOptions {
			#[cfg(feature = "cache")]
			cache,
			#[cfg(feature = "cookies")]
			cookies,
			dns,
			flow_control,
			headers,
			http2,
			#[cfg(feature = "http3")]
			http3,
			local_address,
			pool,
			quirks,
			redirect,
			timeout,
			tls,
			user_agent,
		} = options;

		let quirk_h1_request_streaming = quirks
			.and_then(|quirks| quirks.h1_request_streaming)
			.unwrap_or(false);

		// Local bind address. An explicit value is honoured as-is. Otherwise, on hosts
		// without usable IPv6, bind 0.0.0.0: reqwest binds the QUIC (HTTP/3) socket to the
		// IPv6 wildcard `[::]` by default, which fails to construct on IPv4-only hosts and
		// makes HTTP/3 silently fall back to TCP. Binding 0.0.0.0 there costs nothing (such
		// a host can't use IPv6 for TCP either) and keeps HTTP/3 working.
		let local_address = match local_address {
			Some(address) => Some(address),
			None if !ipv6_wildcard_bindable() => Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
			None => None,
		};

		// `cookies: true` takes the default limits; an options object tunes them. (spec:COOK)
		// The jar is installed on the client by the recipe, so it survives a rebuild
		// (spec:NETCHG#what-the-signal-keeps).
		#[cfg(feature = "cookies")]
		let cookie_jar = cookies.map(|limits| Arc::new(FaithJar::new(limits)));

		let dns = dns.unwrap_or_default();
		// Without Faith's own resolver there is nothing to choose: every name goes to the platform,
		// and no part of the build asks the question.
		#[cfg(feature = "dns")]
		let dns_system = dns.system.unwrap_or(false);
		// Naming servers and asking for the system resolver at once is a contradiction rather than
		// a preference, since the system resolver is not Faith's to point at listed servers
		// (spec:DNS#system-resolver).
		#[cfg(feature = "dns")]
		if dns_system
			&& dns
				.servers
				.as_ref()
				.is_some_and(|servers| !servers.is_empty())
		{
			return Err(FaithError::new(
				FaithErrorKind::Config,
				Some("dns.servers cannot be combined with dns.system".to_string()),
			));
		}
		// Parsed whichever resolver is in use: overrides take effect under the system resolver
		// too (spec:DNS#overrides), and an unparseable address is a construction error either
		// way (spec:AGENT#construction).
		let dns_overrides = dns
			.overrides
			.unwrap_or_default()
			.into_iter()
			.map(|DnsOverride { domain, addresses }| {
				let addresses = addresses
					.into_iter()
					.map(|addr| match SocketAddr::from_str(&addr) {
						Ok(addr) => Ok(addr),
						Err(err) => match IpAddr::from_str(&addr) {
							Ok(IpAddr::V4(ip)) => Ok(SocketAddr::V4(SocketAddrV4::new(ip, 0))),
							Ok(IpAddr::V6(ip)) => {
								Ok(SocketAddr::V6(SocketAddrV6::new(ip, 0, 0, 0)))
							}
							Err(_) => Err(FaithError::new(
								FaithErrorKind::AddressParse,
								Some(format!("{addr:?}: {err}")),
							)),
						},
					})
					.collect::<Result<Vec<_>, FaithError>>()?;
				Ok((domain, addresses))
			})
			.collect::<Result<Vec<_>, FaithError>>()?;

		// Faith owns the hickory resolver rather than leaving it to reqwest's built-in one, so
		// `prefetchDns` can warm the very cache reqwest's requests read (spec:WARM),
		// `networkChanged` can flush it (spec:NETCHG), and `dns.servers` can pick the transport and
		// order each resolver is reached by (spec:DNS#transports). The system resolver
		// (getaddrinfo) has no in-process cache Faith can warm, so no resolver is installed there
		// and `prefetchDns` resolves as a no-op (spec:WARM).
		#[cfg(feature = "dns")]
		let dns_resolver = if dns_system {
			None
		} else {
			// These settings configure Faith's own resolver only, so they are read on the path that
			// builds one rather than validated under the system resolver that ignores them.
			let mut servers = Vec::new();
			for url in dns.servers.unwrap_or_default() {
				servers.push(ServerSpec::parse(&url).map_err(|message| {
					FaithError::new(FaithErrorKind::AddressParse, Some(message))
				})?);
			}
			Some(FaithResolver::new(ResolverSettings {
				servers,
				timeout: dns.timeout.map(|ms| Duration::from_millis(ms.into())),
				ndots: dns.ndots.map(|n| n as usize),
				search_domains: parse_domains(dns.search_domains)
					.map_err(|message| FaithError::new(FaithErrorKind::Config, Some(message)))?,
				hosts_file: dns.hosts_file,
				exempt_domains: parse_domains(dns.exempt_domains)
					.map_err(|message| FaithError::new(FaithErrorKind::Config, Some(message)))?
					.unwrap_or_default(),
				serve_stale: dns.serve_stale.unwrap_or(true),
				max_stale: dns
					.max_stale
					.map_or(DEFAULT_MAX_STALE, |ms| Duration::from_millis(ms.into())),
			}))
		};

		#[cfg(feature = "encoding")]
		let mut default_accept_encoding = None;
		#[cfg(feature = "encoding")]
		let mut default_content_encoding = None;
		let mut has_default_priority = false;
		let mut default_headers = None;
		if let Some(headers) = headers
			&& !headers.is_empty()
		{
			let map = HeaderMap::from_iter(headers.into_iter().filter_map(
				|Header {
				     name,
				     value,
				     sensitive,
				 }| {
					let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
						return None;
					};

					let Ok(mut value) = HeaderValue::from_bytes(value.as_bytes()) else {
						return None;
					};

					if sensitive.unwrap_or(false) {
						value.set_sensitive(true);
					}

					Some((name, value))
				},
			));
			#[cfg(feature = "encoding")]
			{
				default_accept_encoding = map.get(reqwest::header::ACCEPT_ENCODING).cloned();
				default_content_encoding = map.get(reqwest::header::CONTENT_ENCODING).cloned();
			}
			has_default_priority = map.contains_key(PRIORITY);
			default_headers = Some(map);
		}

		// HTTP/2 flow control (spec:FLOW). Adaptive windowing takes over both windows itself, so
		// the explicit sizes are not resolved at all when it's on: reqwest would let the later
		// `http2_adaptive_window` call win regardless, but leaving them out makes the precedence
		// visible here rather than depending on hyper's internal ordering.
		let http2 = http2.unwrap_or_default();
		let http2_adaptive_window = http2.adaptive_window.unwrap_or(false);
		let http2_windows = (!http2_adaptive_window).then(|| {
			resolve_windows(
				flow_control.as_ref(),
				http2.stream_window,
				http2.connection_window,
			)
		});

		#[cfg(feature = "http3")]
		let http3_max_idle_timeout = Duration::from_secs(
			http3
				.as_ref()
				.and_then(|h| h.max_idle_timeout)
				.unwrap_or(30)
				.clamp(1, 120)
				.into(),
		);

		// QUIC flow control (spec:FLOW). quinn's own defaults are a ~1.25MB stream window
		// inside an unbounded connection window: the stream window is the binding constraint
		// on a high-latency link, and the unbounded connection window means a connection with
		// many concurrent requests has no ceiling on what it buffers. Both are set here.
		#[cfg(feature = "http3")]
		let http3_windows = resolve_windows(
			flow_control.as_ref(),
			http3.as_ref().and_then(|h| h.stream_window),
			http3.as_ref().and_then(|h| h.connection_window),
		);

		#[cfg(feature = "http3")]
		let http3_congestion_bbr = matches!(
			http3.as_ref().and_then(|h| h.congestion),
			Some(Http3Congestion::Bbr1)
		);

		#[cfg(feature = "http3")]
		let http3_send_window = http3.as_ref().and_then(|h| h.send_window);

		let pool_idle_timeout = pool
			.as_ref()
			.and_then(|pool| pool.idle_timeout)
			.map(|seconds| Duration::from_secs(seconds.into()));
		// A pool group with no cap set means no limit, which is reqwest's own default (spec:POOL).
		let pool_max_idle_per_host = pool.as_ref().map(|pool| {
			pool.max_idle_per_host
				.and_then(|n| n.try_into().ok())
				.unwrap_or(usize::MAX)
		});

		let connect_timeout = timeout
			.and_then(|t| t.connect)
			.map(|millis| Duration::from_millis(millis.into()));
		let read_timeout = timeout
			.and_then(|t| t.read)
			.map(|millis| Duration::from_millis(millis.into()));
		let total_timeout = timeout
			.and_then(|t| t.total)
			.map(|millis| Duration::from_millis(millis.into()));

		#[cfg(feature = "http3")]
		let tls_early_data = tls.as_ref().and_then(|tls| tls.early_data);
		let tls_required = tls.as_ref().and_then(|tls| tls.required);
		// PEM inputs are parsed here rather than kept as bytes: the parsed forms are what a
		// rebuilt client needs, and a syntax error belongs to construction (spec:AGENT#construction).
		let (tls_identity, tls_extra_roots) = match tls {
			None => (None, Vec::new()),
			Some(tls) => {
				let identity = match &tls.identity {
					None => None,
					Some(identity) => Some(Identity::from_pem(identity).map_err(|err| {
						FaithError::new(FaithErrorKind::PemParse, Some(err.to_string()))
					})?),
				};

				let mut extra_roots = Vec::new();
				for pem in tls.extra_roots.iter().flatten() {
					extra_roots.extend(Certificate::from_pem_bundle(pem).map_err(|err| {
						FaithError::new(FaithErrorKind::PemParse, Some(err.to_string()))
					})?);
				}

				(identity, extra_roots)
			}
		};

		#[cfg(feature = "cache")]
		let http_cache = if let Some(cache) = cache
			&& let Some(store) = cache.store
		{
			let mode = cache.mode.unwrap_or_default().into();
			let options = HttpCacheOptions {
				cache_options: Some(CacheOptions {
					shared: cache.shared.unwrap_or(true),
					ignore_cargo_cult: true,
					..Default::default()
				}),
				..Default::default()
			};
			let store = match store {
				CacheStore::Disk => HttpCacheStore::Disk(CACacheManager {
					path: cache
						.path
						.ok_or_else(|| {
							FaithError::new(FaithErrorKind::Config, Some("missing cache.path"))
						})?
						.into(),
					remove_opts: Default::default(),
				}),
				CacheStore::Memory => HttpCacheStore::Memory(MokaManager::new(
					MokaCacheBuilder::new(cache.capacity.map_or(10_000, |n| n.into())).build(),
				)),
			};

			Some(HttpCacheRecipe {
				mode,
				options,
				store,
			})
		} else {
			None
		};

		// Read outside the `alt_svc_cache` block below because `fetch` needs it too,
		// to keep a rewritten port from looking like a redirect.
		#[cfg(feature = "http3")]
		let h3_follow_advertised_port = http3
			.as_ref()
			.and_then(|o| o.upgrade_follow_advertised_port)
			.unwrap_or(false);
		#[cfg(not(feature = "http3"))]
		let h3_follow_advertised_port = false;

		// The origin knowledge is built once and outlives every client the agent builds: it is
		// the agent's own, and a network change edits it rather than replacing it (spec:NETCHG).
		#[cfg(feature = "http3")]
		let (alt_svc_cache, h3_upgrade) = {
			let http3_opts = http3.as_ref();
			let enabled = http3_opts.and_then(|o| o.upgrade_enabled).unwrap_or(true);

			let advertised_ttl = Duration::from_secs(
				http3_opts
					.and_then(|o| o.upgrade_advertised_ttl)
					.unwrap_or(86400)
					.into(),
			);
			let confirmed_ttl = Duration::from_secs(
				http3_opts
					.and_then(|o| o.upgrade_confirmed_ttl)
					.unwrap_or(86400)
					.into(),
			);
			let failed_ttl = Duration::from_secs(
				http3_opts
					.and_then(|o| o.upgrade_failed_ttl)
					.unwrap_or(300)
					.into(),
			);
			let failed_max_ttl = Duration::from_secs(
				http3_opts
					.and_then(|o| o.upgrade_failed_max_ttl)
					.unwrap_or(3600)
					.into(),
			);
			let capacity = http3_opts
				.and_then(|o| o.upgrade_cache_capacity)
				.unwrap_or(10_000)
				.into();
			let cancel_strikes = http3_opts
				.and_then(|o| o.upgrade_cancel_strikes)
				.unwrap_or(3);
			let attempt_timeout = match http3_opts
				.and_then(|o| o.upgrade_attempt_timeout)
				.unwrap_or(60_000)
			{
				0 => None,
				millis => Some(Duration::from_millis(millis.into())),
			};
			let probe = http3_opts.and_then(|o| o.upgrade_probe).unwrap_or(true);
			let probe_timeout = match http3_opts
				.and_then(|o| o.upgrade_probe_timeout)
				.unwrap_or(5_000)
			{
				0 => None,
				millis => Some(Duration::from_millis(millis.into())),
			};
			let slow_factor = http3_opts
				.and_then(|o| o.upgrade_slow_factor)
				.unwrap_or(2.5);
			let slow_ttl = Duration::from_secs(
				http3_opts
					.and_then(|o| o.upgrade_slow_ttl)
					.unwrap_or(600)
					.into(),
			);

			let cache = Arc::new(AltSvcCache::new(AltSvcCacheConfig {
				advertised_ttl,
				confirmed_ttl,
				failed_ttl,
				failed_max_ttl,
				capacity,
				cancel_strikes,
				strike_window: Duration::from_secs(60),
				follow_advertised_port: h3_follow_advertised_port,
				// The single-flight claim must outlive the probe it covers, so
				// an aborted probe frees its origin without a report; without a
				// probe deadline, the QUIC idle timeout (max 120s) is the bound.
				probe_ttl: probe_timeout
					.map_or(Duration::from_secs(125), |t| t + Duration::from_secs(5)),
				slow_factor,
				slow_ttl,
			}));

			if let Some(hints) = http3_opts.and_then(|o| o.hints.as_ref()) {
				for hint in hints {
					cache.add_hint(&hint.host, hint.port);
				}
			}

			(
				Some(cache),
				H3UpgradeRecipe {
					enabled,
					attempt_timeout,
					probe,
					probe_timeout,
				},
			)
		};

		let recipe = ClientRecipe {
			user_agent: user_agent.unwrap_or_else(|| USER_AGENT.to_owned()),
			local_address,
			default_headers,
			#[cfg(feature = "dns")]
			dns_system,
			dns_overrides,
			http2_adaptive_window,
			http2_windows,
			#[cfg(feature = "http3")]
			http3_max_idle_timeout,
			#[cfg(feature = "http3")]
			http3_windows,
			#[cfg(feature = "http3")]
			http3_congestion_bbr,
			#[cfg(feature = "http3")]
			http3_send_window,
			pool_idle_timeout,
			pool_max_idle_per_host,
			redirect,
			connect_timeout,
			read_timeout,
			total_timeout,
			#[cfg(feature = "http3")]
			tls_early_data,
			tls_identity,
			tls_required,
			tls_extra_roots,
			node_env: NodeEnvRecipe::read(),
			#[cfg(feature = "cache")]
			http_cache,
			#[cfg(feature = "http3")]
			h3_upgrade,
		};

		let settings = AgentSettings {
			h3_follow_advertised_port,
			quirk_h1_request_streaming,
			#[cfg(feature = "encoding")]
			default_accept_encoding,
			#[cfg(feature = "encoding")]
			default_content_encoding,
			has_default_priority,
		};

		Self::build(
			recipe,
			settings,
			#[cfg(feature = "cookies")]
			cookie_jar,
			#[cfg(feature = "dns")]
			dns_resolver,
			#[cfg(feature = "http3")]
			alt_svc_cache,
		)
	}

	/// An agent with default options.
	pub fn new() -> Result<Self, FaithError> {
		Self::from_options(AgentOptions::default())
	}

	/// Build an agent a setting at a time. See [the builder module](crate::builder).
	pub fn builder() -> crate::options::AgentOptionsBuilder {
		crate::options::AgentOptions::builder()
	}

	/// Build an agent from a validated recipe.
	///
	/// The recipe is what a client is built from, and the settings are what each request consults;
	/// validating whatever a caller expressed them as belongs to the surface that took it.
	pub(crate) fn build(
		recipe: ClientRecipe,
		settings: AgentSettings,
		#[cfg(feature = "cookies")] cookie_jar: Option<Arc<FaithJar>>,
		#[cfg(feature = "dns")] dns_resolver: Option<FaithResolver>,
		#[cfg(feature = "http3")] alt_svc_cache: Option<Arc<AltSvcCache>>,
	) -> Result<Self, FaithError> {
		let conn_timeout = recipe.conn_timeout();
		let built = recipe.build(
			#[cfg(feature = "cookies")]
			cookie_jar.as_ref(),
			#[cfg(feature = "dns")]
			dns_resolver.as_ref(),
			#[cfg(feature = "http3")]
			alt_svc_cache.as_ref(),
		)?;

		// Only now do all three exist: the resolver is built before the cache, and the prober
		// holds a client that holds the resolver, so this is the earliest the loop can be closed
		// (spec:DNS#https-records).
		#[cfg(all(feature = "http3", feature = "dns"))]
		install_https_sink(
			dns_resolver.as_ref(),
			alt_svc_cache.as_ref(),
			built.prober.as_ref(),
			recipe.h3_upgrade.enabled,
		);

		Ok(Self {
			live: Arc::new(RwLock::new(Some(Live {
				client: built.client,
				raw_client: built.raw_client,
				#[cfg(feature = "dns")]
				dns_resolver,
				#[cfg(feature = "http3")]
				alt_svc_cache,
				#[cfg(feature = "http3")]
				h3_prober: built.prober,
			}))),
			// A warm-up connection is warm only as long as the pool keeps it idle, so the record
			// that an origin is warm expires with that same window.
			warmed: MokaCache::builder().time_to_live(conn_timeout).build(),
			// A safety TTL well past any reasonable warm-up, so a claim that never gets released
			// (a warm-up whose task is dropped) frees the origin rather than wedging it.
			warming: MokaCache::builder()
				.time_to_live(Duration::from_secs(300))
				.build(),
			warm_generation: Default::default(),
			#[cfg(feature = "cookies")]
			cookie_jar,
			stats: Default::default(),
			#[cfg(feature = "connection-tracking")]
			conn_tracker: ConnectionTracker::new(conn_timeout),
			h3_follow_advertised_port: settings.h3_follow_advertised_port,
			#[cfg(feature = "http3")]
			h3_upgrade_enabled: recipe.h3_upgrade.enabled,
			quirk_h1_request_streaming: settings.quirk_h1_request_streaming,
			#[cfg(feature = "encoding")]
			default_accept_encoding: settings.default_accept_encoding,
			#[cfg(feature = "encoding")]
			default_content_encoding: settings.default_content_encoding,
			has_default_priority: settings.has_default_priority,
			recipe: Arc::new(recipe),
		})
	}
}
