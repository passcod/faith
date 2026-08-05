use std::{
	fmt::Debug,
	net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket},
	str::FromStr as _,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use napi::bindgen_prelude::within_runtime_if_available;

use http_cache_reqwest::{
	CACacheManager, Cache, CacheOptions, HttpCache, HttpCacheOptions, MokaCacheBuilder, MokaManager,
};
use napi::{Either, Env, bindgen_prelude::Buffer};
use napi_derive::napi;
use reqwest::{
	Certificate, Client, Identity, Url,
	cookie::{CookieStore, Jar},
	header::{HeaderMap, HeaderName, HeaderValue},
	redirect::Policy,
};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

#[cfg(feature = "http3")]
use crate::alt_svc::{AltSvcCache, AltSvcMiddleware};
use crate::{
	conn_tracker::{ConnectionInfo, ConnectionTracker},
	error::{FaithError, FaithErrorKind},
	options::RequestCacheMode,
};

#[napi]
pub const FAITH_VERSION: &str = env!("CARGO_PKG_VERSION");
#[napi]
pub const REQWEST_VERSION: &str = env!("REQWEST_VERSION");
/// Custom user agent string.
///
/// Default: `Faith/{version} reqwest/{version}`.
///
/// You may use the `USER_AGENT` constant if you wish to prepend your own agent to the default, e.g.
///
/// ```javascript
/// import { Agent, USER_AGENT } from '@passcod/faith';
/// const agent = new Agent({
///   userAgent: `YourApp/1.2.3 ${USER_AGENT}`,
/// });
/// ```
#[napi]
pub const USER_AGENT: &str = concat!(
	"Faith/",
	env!("CARGO_PKG_VERSION"),
	" reqwest/",
	env!("REQWEST_VERSION")
);

/// Whether this host can bind the IPv6 wildcard (`[::]`).
///
/// This is tested using the exact operation reqwest performs when creating the QUIC
/// endpoint with no explicit local address, so it predicts whether the default
/// QUIC bind will succeed. The result is memoised for the life of the process; while
/// IPv6 bindability can in principle change at runtime, this is considered an
/// acceptable tradeoff for performance and simplicity.
fn ipv6_wildcard_bindable() -> bool {
	use std::sync::OnceLock;
	static BINDABLE: OnceLock<bool> = OnceLock::new();
	*BINDABLE.get_or_init(|| {
		UdpSocket::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)).is_ok()
	})
}

/// Applies the Node.js networking environment variables to a reqwest client
/// builder, matching how Node.js honours them for its own clients. This runs for
/// every agent, so `fetch()` behaves like Node's built-in fetch out of the box.
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
fn apply_node_env(mut client: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
	if let Ok(path) = std::env::var("NODE_EXTRA_CA_CERTS")
		&& !path.is_empty()
		&& let Ok(bytes) = std::fs::read(&path)
		&& let Ok(certs) = Certificate::from_pem_bundle(&bytes)
	{
		client = client.tls_certs_merge(certs);
	}

	if std::env::var("NODE_TLS_REJECT_UNAUTHORIZED").as_deref() == Ok("0") {
		client = client.danger_accept_invalid_certs(true);
	}

	if std::env::var("NODE_USE_ENV_PROXY").as_deref() == Ok("0") {
		client = client.no_proxy();
	}

	client
}

#[napi(string_enum)]
#[derive(Debug, Clone, Copy)]
pub enum CacheStore {
	#[napi(value = "disk")]
	Disk,

	#[napi(value = "memory")]
	Memory,
}

/// Settings related to the HTTP cache. This is a nested object.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentCacheOptions {
	/// Which cache store to use: either `disk` or `memory`.
	///
	/// Default: none (cache disabled).
	pub store: Option<CacheStore>,
	/// If `cache.store: "memory"`, the maximum amount of items stored.
	///
	/// Default: 10_000.
	pub capacity: Option<u32>,
	/// Default cache mode. This is the same as [`FetchOptions.cache`](#fetchoptionscache), and is used if
	/// no cache mode is set on a request.
	///
	/// Default: `"default"`.
	pub mode: Option<RequestCacheMode>,
	/// If `cache.store: "disk"`, then this is the path at which the cache data is. Must be writeable.
	///
	/// Required if `cache.store: "disk"`.
	pub path: Option<String>,
	/// If `true`, then the response is evaluated from a perspective of a shared cache (i.e. `private` is
	/// not cacheable and `s-maxage` is respected). If `false`, then the response is evaluated from a
	/// perspective of a single-user cache (i.e. `private` is cacheable and `s-maxage` is ignored).
	/// `shared: true` is required for proxies and multi-user caches.
	///
	/// Default: true.
	pub shared: Option<bool>,
}

#[napi(object)]
#[derive(Debug, Clone)]
pub struct DnsOverride {
	pub domain: String,
	pub addresses: Vec<String>,
}

/// Settings related to DNS. This is a nested object.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentDnsOptions {
	/// Use the system's DNS (via `getaddrinfo` or equivalent) rather than Fáith's own DNS client (based on
	/// [Hickory]). If you experience issues with DNS where Fáith does not work but e.g. curl or native
	/// fetch does, this should be your first port of call.
	///
	/// Enabling this also disables Happy Eyeballs (for IPv6 / IPv4 best-effort resolution), the in-memory
	/// DNS cache, and may lead to worse performance even discounting the cache.
	///
	/// Default: false.
	///
	/// [Hickory]: https://hickory-dns.org/
	pub system: Option<bool>,
	/// Override DNS resolution for specific domains. This takes effect even with `dns.system: true`.
	///
	/// Will throw if addresses are in invalid formats. You may provide a port number as part of the
	/// address, it will default to port 0 otherwise, which will select the conventional port for the
	/// protocol in use (e.g. 80 for plaintext HTTP). If the URL passed to `fetch()` has an explicit port
	/// number, that one will be used instead. Resolving a domain to an empty `addresses` array effectively
	/// blocks that domain from this agent.
	///
	/// Default: no overrides.
	pub overrides: Option<Vec<DnsOverride>>,
}

/// Sets the default headers for every request.
///
/// If header names or values are invalid, they are silently omitted.
/// Sensitive headers (e.g. `Authorization`) should be marked.
///
/// Default: none.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct Header {
	pub name: String,
	pub value: String,
	pub sensitive: Option<bool>,
}

#[napi(string_enum)]
#[derive(Debug, Clone, Copy, Default)]
pub enum Http3Congestion {
	#[napi(value = "cubic")]
	#[default]
	Cubic,

	#[napi(value = "bbr1")]
	Bbr1,
}

/// A hint that HTTP/3 is available at a specific host and port. This pre-populates the Alt-Svc
/// cache so the first request to this host will attempt HTTP/3 immediately.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct Http3Hint {
	/// The hostname (e.g., "example.com").
	pub host: String,
	/// The port number (e.g., 443).
	pub port: u16,
}

/// Settings related to HTTP/3. This is a nested object.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentHttp3Options {
	/// The congestion control algorithm. The default is `cubic`, which is the same used in TCP in the
	/// Linux stack. It's fair for all traffic, but not the most optimal, especially for networks with
	/// a lot of available bandwidth, high latency, or a lot of packet loss. Cubic reacts to packet loss by
	/// dropping the speed by 30%, and takes a long time to recover. BBR instead tries to maximise
	/// bandwidth use and optimises for round-trip time, while ignoring packet loss.
	///
	/// In some networks, BBR can lead to pathological degradation of overall network conditions, by
	/// flooding the network by up to **100 times** more retransmissions. This is fixed in BBRv2 and BBRv3,
	/// but Fáith (or rather its underlying QUIC library quinn, [does not implement those yet][2]).
	///
	/// [2]: https://github.com/quinn-rs/quinn/issues/1254
	///
	/// Default: `cubic`. Accepted values: `cubic`, `bbr1`.
	pub congestion: Option<Http3Congestion>,
	/// Maximum duration of inactivity to accept before timing out the connection, in seconds. Note that
	/// this only sets the timeout on this side of the connection: the true idle timeout is the _minimum_
	/// of this and the peer's own max idle timeout. While the underlying library has no limits, Fáith
	/// defines bounds for safety: minimum 1 second, maximum 2 minutes (120 seconds).
	///
	/// Default: 30.
	pub max_idle_timeout: Option<u8>,
	/// Whether HTTP/3 upgrade via Alt-Svc is enabled. When enabled, the agent will track Alt-Svc
	/// headers from responses and automatically upgrade subsequent requests to HTTP/3 when available.
	///
	/// Default: true.
	pub upgrade_enabled: Option<bool>,
	/// How long (in seconds) to cache an Alt-Svc advertisement before the first HTTP/3 attempt.
	/// This is overridden by the `ma` (max-age) parameter in the Alt-Svc header if present.
	///
	/// Default: 86400 (24 hours).
	pub upgrade_advertised_ttl: Option<u32>,
	/// How long (in seconds) to cache a confirmed working HTTP/3 connection.
	///
	/// Default: 86400 (24 hours).
	pub upgrade_confirmed_ttl: Option<u32>,
	/// How long (in seconds) to cache a failed HTTP/3 attempt. During this time, no HTTP/3
	/// upgrades will be attempted for the origin, even if the server sends Alt-Svc headers.
	///
	/// Default: 300 (5 minutes).
	pub upgrade_failed_ttl: Option<u32>,
	/// How many consecutive cancelled HTTP/3 attempts, within a 60-second window,
	/// demote an origin back to TCP.
	///
	/// Fáith normally learns that HTTP/3 is broken from a failed attempt. A request
	/// cancelled via `AbortSignal` never produces that signal, so without this an
	/// origin whose UDP path breaks keeps being retried over HTTP/3 for as long as
	/// the Alt-Svc entry lives. Cancellations are treated as weak evidence: only a
	/// sustained run of them demotes the origin, and any successful HTTP/3 response
	/// resets the count.
	///
	/// Strikes must land within about a minute of each other to count towards a
	/// run. A retry loop whose backoff exceeds that window never accumulates one,
	/// so callers with a long backoff should set this to 1 for immediate demotion
	/// on the first cancelled attempt.
	///
	/// One fault neither this nor `upgradeAttemptTimeout` catches: a path that
	/// carries small datagrams but drops full-size ones (an MTU blackhole, say).
	/// Response headers still arrive, so the attempt resolves and every mechanism
	/// here counts it a success — the transfer then stalls partway through the
	/// body, where nothing is watching. `maxIdleTimeout` or the request's own
	/// timeout is what ends such a request, and the origin stays on HTTP/3.
	///
	/// Set to 0 to disable, so only real HTTP/3 errors demote an origin.
	///
	/// Default: 3.
	pub upgrade_cancel_strikes: Option<u32>,
	/// Ceiling on how long an HTTP/3 attempt may take to resolve before it is
	/// given up on and the request is retried over TCP, in **milliseconds**.
	///
	/// Note the unit: the other `upgrade*` settings are in seconds, but this one
	/// is in milliseconds to match the `timeout` settings, because useful values
	/// are sub-second.
	///
	/// This bounds the wait for response headers, not the response body, so a slow
	/// body is unaffected.
	///
	/// The default is high, but not unconditionally inert: `maxIdleTimeout` is
	/// configurable up to 120 seconds, and above 60 seconds this deadline becomes
	/// the effective ceiling. Even below that, "QUIC's own idle timeout fires
	/// first" only holds while the connection is idle — a transfer still running
	/// past this deadline keeps the connection active, so no idle timeout is
	/// coming to end it.
	///
	/// On expiry the request is retried over TCP, which means it is re-sent: a
	/// timeout often means the server is still processing, so a slow
	/// non-idempotent request (a POST, say) can end up delivered twice. Lowering
	/// this value trades that double-submission risk for faster recovery when a
	/// UDP path breaks. Anyone setting it low should confirm their slowest
	/// legitimate time to response headers fits well inside the budget.
	///
	/// Set to 0 to disable, so an HTTP/3 attempt is bounded only by the QUIC idle
	/// timeout and the request's own timeout.
	///
	/// Default: 60000 (60 seconds).
	pub upgrade_attempt_timeout: Option<u32>,
	/// Connect to the port a server advertises HTTP/3 on, even when it differs from
	/// the origin's own port. **This is not standards-compliant**; it is off by
	/// default.
	///
	/// An `Alt-Svc` advertisement names a network endpoint for the origin, so
	/// honouring one correctly means connecting to that endpoint while still
	/// sending the *origin's* authority. reqwest cannot express that — it derives
	/// the HTTP/3 connect target from the request URI's authority (tracked
	/// upstream as [reqwest#1138](https://github.com/seanmonstar/reqwest/issues/1138)).
	/// So by default Fáith does not upgrade at all when the advertised port
	/// differs, rather than guessing that the origin's own port also speaks
	/// HTTP/3.
	///
	/// Setting this to `true` upgrades anyway, by rewriting the request's port to
	/// the advertised one. That gets HTTP/3 working today against servers you
	/// control, at the cost of three deviations you should be aware of:
	///
	/// - The request's `Host`/`:authority` carries the advertised port instead of
	///   the origin's, which [RFC 7838](https://www.rfc-editor.org/rfc/rfc7838)
	///   forbids. Servers that route on authority may misroute or reject; servers
	///   that ignore it are unaffected.
	/// - `response.url` reports the port actually connected to.
	/// - `redirected` ignores port differences, since the rewritten port would
	///   otherwise look like a redirect on every request.
	///
	/// TLS is unaffected: certificates are still validated against the origin's
	/// hostname. Only the port changes.
	///
	/// Default: `false`.
	pub upgrade_follow_advertised_port: Option<bool>,
	/// Maximum number of origins to track in the Alt-Svc cache.
	///
	/// Default: 10000.
	pub upgrade_cache_capacity: Option<u32>,
	/// Hints for hosts that are known to support HTTP/3. These are added to the Alt-Svc cache
	/// on agent initialization, so the first request to these hosts will attempt HTTP/3.
	pub hints: Option<Vec<Http3Hint>>,
}

/// Settings related to the connection pool. This is a nested object.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentPoolOptions {
	/// How many seconds of inactivity before a connection is closed.
	///
	/// Default: 90 seconds.
	pub idle_timeout: Option<u32>,
	/// The maximum amount of idle connections per host to allow in the pool. Connections will be closed
	/// to keep the idle connections (per host) under that number.
	///
	/// Default: `null` (no limit).
	pub max_idle_per_host: Option<u32>,
}

/// Determines the behavior in case the server replies with a redirect status.
/// One of the following values:
///
/// - `follow`: automatically follow redirects. Fáith limits this to 10 redirects.
/// - `error`: reject the promise with a network error when a redirect status is returned.
/// - ~~`manual`~~: not supported.
/// - `stop`: (Fáith custom) don't follow any redirects, return the responses.
///
/// Defaults to `follow`.
#[napi(string_enum)]
#[derive(Debug, Clone, Copy, Default)]
pub enum Redirect {
	#[napi(value = "follow")]
	#[default]
	Follow,

	#[napi(value = "error")]
	Error,

	#[napi(value = "manual")]
	Manual,

	#[napi(value = "stop")]
	Stop,
}

/// Timeouts for requests made with this agent. This is a nested object.
#[napi(object)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentTimeoutOptions {
	/// Set a timeout for only the connect phase, in milliseconds.
	///
	/// Default: none.
	pub connect: Option<u32>,
	/// Set a timeout for read operations, in milliseconds.
	///
	/// The timeout applies to each read operation, and resets after a successful read. This is more
	/// appropriate for detecting stalled connections when the size isn't known beforehand.
	///
	/// Default: none.
	pub read: Option<u32>,
	/// Set a timeout for the entire request-response cycle, in milliseconds.
	///
	/// The timeout applies from when the request starts connecting until the response body has finished.
	/// Also considered a total deadline.
	///
	/// Default: none.
	pub total: Option<u32>,
}

/// Settings related to the connection pool. This is a nested object.
#[napi(object)]
#[derive(Default)]
pub struct AgentTlsOptions {
	/// Enable TLS 1.3 Early Data. Early data is an optimisation where the client sends the first packet
	/// of application data alongside the opening packet of the TLS handshake. That can enable the server
	/// to answer faster, improving latency by up to one round-trip. However, Early Data has significant
	/// security implications: it's vulnerable to replay attacks and has weaker forward secrecy. It should
	/// really only be used for static assets or to squeeze out the last drop of performance for endpoints
	/// that are replay-safe.
	///
	/// Default: false.
	pub early_data: Option<bool>,
	/// Provide a PEM-formatted certificate and private key to present as a TLS client certificate (also
	/// called mutual TLS or mTLS) authentication.
	///
	/// The input should contain a PEM encoded private key and at least one PEM encoded certificate. The
	/// private key must be in RSA, SEC1 Elliptic Curve or PKCS#8 format. This is one of the few options
	/// that will cause the `Agent` constructor to throw if the input is in the wrong format.
	pub identity: Option<Either<Buffer, String>>,
	/// Disables plain-text HTTP.
	///
	/// Default: false.
	pub required: Option<bool>,
	/// Additional PEM-formatted root certificates to trust, on top of the platform's
	/// trust store. Each entry may be a PEM bundle containing multiple certificates.
	///
	/// This is mainly useful for connecting to servers with self-signed or private-CA
	/// certificates, such as internal services or local test servers. This is one of the
	/// few options that will cause the `Agent` constructor to throw if the input is in
	/// the wrong format.
	pub extra_roots: Option<Vec<Either<Buffer, String>>>,
}

impl Debug for AgentTlsOptions {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AgentTlsOptions")
			.field("early_data", &self.early_data)
			.field("identity", &"[sensitive]")
			.field("required", &self.required)
			.field("extra_roots", &self.extra_roots.as_ref().map(|r| r.len()))
			.finish()
	}
}

impl Clone for AgentTlsOptions {
	fn clone(&self) -> Self {
		Self {
			early_data: self.early_data.clone(),
			identity: self.identity.as_ref().map(|either| match either {
				Either::A(buf) => Either::A(Buffer::from(buf.as_ref())),
				Either::B(string) => Either::B(string.clone()),
			}),
			required: self.required.clone(),
			extra_roots: self.extra_roots.as_ref().map(|roots| {
				roots
					.iter()
					.map(|either| match either {
						Either::A(buf) => Either::A(Buffer::from(buf.as_ref())),
						Either::B(string) => Either::B(string.clone()),
					})
					.collect()
			}),
		}
	}
}

#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentOptions {
	/// Settings related to the HTTP cache. This is a nested object.
	pub cache: Option<AgentCacheOptions>,
	/// Enable a persistent cookie store for the agent. Cookies received in responses will be preserved and
	/// included in additional requests.
	///
	/// Default: `false`.
	///
	/// You may use `agent.getCookie(url: string)` and `agent.addCookie(url: string, value: string)` to add
	/// and retrieve cookies from the store.
	pub cookies: Option<bool>,
	/// Settings related to DNS. This is a nested object.
	pub dns: Option<AgentDnsOptions>,
	/// Sets the default headers for every request.
	///
	/// If header names or values are invalid, they are silently omitted.
	/// Sensitive headers (e.g. `Authorization`) should be marked.
	///
	/// Default: none.
	pub headers: Option<Vec<Header>>,
	/// Settings related to HTTP/3. This is a nested object.
	pub http3: Option<AgentHttp3Options>,
	/// Bind outgoing sockets to this local IP address before connecting.
	///
	/// This also selects the address family of the HTTP/3 (QUIC) socket. By default that
	/// socket binds the IPv6 wildcard (`[::]`), which fails on hosts without usable IPv6 —
	/// there, HTTP/3 silently falls back to TCP. Fáith detects that case automatically and
	/// binds `0.0.0.0` instead, so you normally don't need to set this; provide it only to
	/// force a specific source address. Throws if the value does not parse as an IP address.
	///
	/// Default: unset (IPv6 wildcard for QUIC where available, else `0.0.0.0`).
	pub local_address: Option<String>,
	/// Settings related to the connection pool. This is a nested object.
	pub pool: Option<AgentPoolOptions>,
	/// Determines the behavior in case the server replies with a redirect status.
	pub redirect: Option<Redirect>,
	/// Timeouts for requests made with this agent. This is a nested object.
	pub timeout: Option<AgentTimeoutOptions>,
	/// Settings related to the connection pool. This is a nested object.
	pub tls: Option<AgentTlsOptions>,
	/// Custom user agent string.
	///
	/// Default: `Faith/{version} reqwest/{version}`.
	pub user_agent: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct InnerAgentStats {
	pub requests_sent: AtomicU64,
	pub responses_received: AtomicU64,
	pub bodies_started: AtomicU64,
	pub bodies_finished: AtomicU64,
}

#[napi]
#[derive(Debug, Clone, Default)]
pub struct AgentStats {
	pub requests_sent: i64,
	pub responses_received: i64,
	/// Number of response body streams that have been started (converted from raw body to stream).
	/// This happens when `.body`, `.text()`, `.json()`, `.bytes()`, or similar methods are called.
	pub bodies_started: i64,
	/// Number of response body streams that have been fully consumed.
	/// When `bodies_started - bodies_finished > 0`, there are bodies holding connections open.
	pub bodies_finished: i64,
}

/// The `Agent` interface of the Fáith API represents an instance of an HTTP client. Each `Agent` has
/// its own options, connection pool, caches, etc. There are also conveniences such as `headers` for
/// setting default headers on all requests done with the agent, and statistics collected by the agent.
///
/// Re-using connections between requests is a significant performance improvement: not only because
/// the TCP and TLS handshake is only performed once across many different requests, but also because
/// the DNS lookup doesn't need to occur for subsequent requests on the same connection. Depending on
/// DNS technology (DoH and DoT add a whole separate handshake to the process) and overall latency,
/// this can not only speed up requests on average, but also reduce system load.
///
/// For this reason, and also because in browsers this behaviour is standard, **all** requests with
/// Fáith use an `Agent`. For `fetch()` calls that don't specify one explicitly, a global agent with
/// default options is created on first use.
///
/// There are a lot more options that could be exposed here; if you want one, open an issue.
#[napi]
#[derive(Debug, Clone)]
pub struct Agent {
	/// `None` once [`Agent::close`] has been called. The heavy resources
	/// (connection pool, DNS resolver, background tasks) live inside this
	/// client, so dropping it is what actually releases them.
	pub(crate) client: Option<ClientWithMiddleware>,
	pub(crate) cookie_jar: Option<Arc<Jar>>,
	pub(crate) stats: Arc<InnerAgentStats>,
	pub(crate) conn_tracker: Arc<ConnectionTracker>,
	#[cfg(feature = "http3")]
	#[allow(dead_code)]
	pub(crate) alt_svc_cache: Option<Arc<AltSvcCache>>,
	/// Mirrors `http3.upgradeFollowAdvertisedPort`. Lives here because `fetch` needs
	/// it to stop a rewritten port from being reported as a redirect.
	pub(crate) h3_follow_advertised_port: bool,
}

#[napi]
impl Agent {
	pub fn new() -> Result<Self, FaithError> {
		Self::with_options(AgentOptions::default())
	}

	pub fn with_options(options: AgentOptions) -> Result<Self, FaithError> {
		// Wrap in tokio runtime context for HTTP/3 endpoint initialization.
		// Quinn's Endpoint::client() requires a tokio runtime to be available.
		within_runtime_if_available(|| Self::with_options_inner(options))
	}

	fn with_options_inner(options: AgentOptions) -> Result<Self, FaithError> {
		let mut client = Client::builder()
			.tls_info(true)
			.tls_sslkeylogfile(true)
			.user_agent(options.user_agent.as_deref().unwrap_or(USER_AGENT));

		// Local bind address. An explicit value is honoured as-is. Otherwise, on hosts
		// without usable IPv6, bind 0.0.0.0: reqwest binds the QUIC (HTTP/3) socket to the
		// IPv6 wildcard `[::]` by default, which fails to construct on IPv4-only hosts and
		// makes HTTP/3 silently fall back to TCP. Binding 0.0.0.0 there costs nothing (such
		// a host can't use IPv6 for TCP either) and keeps HTTP/3 working.
		let local_address = match &options.local_address {
			Some(addr) => Some(IpAddr::from_str(addr).map_err(|err| {
				FaithError::new(
					FaithErrorKind::AddressParse,
					Some(format!("{addr:?}: {err}")),
				)
			})?),
			None if !ipv6_wildcard_bindable() => Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
			None => None,
		};
		if let Some(ip) = local_address {
			client = client.local_address(ip);
		}

		let cookie_jar = if options.cookies.unwrap_or(false) {
			let jar = Arc::new(Jar::default());
			client = client.cookie_provider(jar.clone());
			Some(jar)
		} else {
			None
		};

		if let Some(dns) = options.dns {
			if dns.system.unwrap_or(false) {
				client = client.no_hickory_dns();
			} else {
				for DnsOverride { domain, addresses } in dns.overrides.unwrap_or_default() {
					client = client.resolve_to_addrs(
						&domain,
						&addresses
							.into_iter()
							.map(|addr| match SocketAddr::from_str(&addr) {
								Ok(addr) => Ok(addr),
								Err(err) => match IpAddr::from_str(&addr) {
									Ok(IpAddr::V4(ip)) => {
										Ok(SocketAddr::V4(SocketAddrV4::new(ip, 0)))
									}
									Ok(IpAddr::V6(ip)) => {
										Ok(SocketAddr::V6(SocketAddrV6::new(ip, 0, 0, 0)))
									}
									Err(_) => Err(FaithError::new(
										FaithErrorKind::AddressParse,
										Some(format!("{addr:?}: {err}")),
									)),
								},
							})
							.collect::<Result<Vec<_>, FaithError>>()?,
					)
				}
			}
		}

		if let Some(headers) = options.headers
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
			client = client.default_headers(map);
		}

		#[cfg(feature = "http3")]
		{
			let idle_timeout = options
				.http3
				.as_ref()
				.and_then(|h| h.max_idle_timeout)
				.unwrap_or(30);
			client = client
				.http3_max_idle_timeout(Duration::from_secs(idle_timeout.min(120).max(1).into()));

			if let Some(ref http3) = options.http3 {
				if let Some(Http3Congestion::Bbr1) = http3.congestion {
					client = client.http3_congestion_bbr();
				}
			}
		}

		let mut conn_timeout = Duration::from_secs(90); // default from reqwest
		if let Some(pool) = options.pool {
			if let Some(seconds) = pool.idle_timeout {
				let dur = Duration::from_secs(seconds.max(0).into());
				conn_timeout = dur;
				client = client.pool_idle_timeout(Some(dur));
			}

			client = client.pool_max_idle_per_host(
				pool.max_idle_per_host
					.and_then(|n| n.try_into().ok())
					.unwrap_or(usize::MAX),
			)
		}

		if let Some(redir) = options.redirect {
			match redir {
				// follow is the default, and we ignore manual
				Redirect::Follow | Redirect::Manual => {}
				Redirect::Error => {
					client = client.redirect(Policy::custom(|attempt| {
						attempt.error(Box::new(FaithError::from(FaithErrorKind::Redirect)))
					}));
				}
				Redirect::Stop => {
					client = client.redirect(Policy::none());
				}
			}
		}

		if let Some(timeouts) = options.timeout {
			if let Some(millis) = timeouts.connect {
				client = client.connect_timeout(Duration::from_millis(millis.into()));
			}

			if let Some(millis) = timeouts.read {
				client = client.read_timeout(Duration::from_millis(millis.into()));
			}

			if let Some(millis) = timeouts.total {
				client = client.timeout(Duration::from_millis(millis.into()));
			}
		}

		if let Some(tls) = options.tls {
			#[cfg(feature = "http3")]
			if let Some(early_data) = tls.early_data {
				client = client.tls_early_data(early_data);
			}

			if let Some(identity) = tls.identity {
				client = client.identity(
					Identity::from_pem(match &identity {
						Either::A(buf) => buf.as_ref(),
						Either::B(string) => string.as_bytes(),
					})
					.map_err(|err| {
						FaithError::new(FaithErrorKind::PemParse, Some(err.to_string()))
					})?,
				);
			}

			if let Some(https_only) = tls.required {
				client = client.https_only(https_only);
			}

			if let Some(extra_roots) = tls.extra_roots {
				for pem in &extra_roots {
					let bytes = match pem {
						Either::A(buf) => buf.as_ref(),
						Either::B(string) => string.as_bytes(),
					};
					let certs = Certificate::from_pem_bundle(bytes).map_err(|err| {
						FaithError::new(FaithErrorKind::PemParse, Some(err.to_string()))
					})?;
					client = client.tls_certs_merge(certs);
				}
			}
		}

		client = apply_node_env(client);

		let reqwest_client = client
			.build()
			.map_err(|e| FaithError::new(FaithErrorKind::Config, Some(format!("{e:?}"))))?;
		let mut client = ClientBuilder::new(reqwest_client.clone());

		// Read outside the `alt_svc_cache` block below because `fetch` needs it too,
		// to keep a rewritten port from looking like a redirect.
		#[cfg(feature = "http3")]
		let h3_follow_advertised_port = options
			.http3
			.as_ref()
			.and_then(|o| o.upgrade_follow_advertised_port)
			.unwrap_or(false);
		#[cfg(not(feature = "http3"))]
		let h3_follow_advertised_port = false;

		#[cfg(feature = "http3")]
		let (alt_svc_cache, alt_svc_middleware) = {
			let http3_opts = options.http3.as_ref();
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

			let cache = Arc::new(AltSvcCache::new(
				advertised_ttl,
				confirmed_ttl,
				failed_ttl,
				capacity,
				cancel_strikes,
				Duration::from_secs(60),
				h3_follow_advertised_port,
			));

			if let Some(hints) = http3_opts.and_then(|o| o.hints.as_ref()) {
				for hint in hints {
					cache.add_hint(&hint.host, hint.port);
				}
			}

			let middleware = AltSvcMiddleware::new(cache.clone(), enabled, attempt_timeout);

			// Registered below rather than here — see the note at the registration.
			(Some(cache), middleware)
		};

		if let Some(cache) = options.cache
			&& let Some(store) = cache.store
		{
			let mode = cache.mode.unwrap_or_default().into();
			let cache_options = HttpCacheOptions {
				cache_options: Some(CacheOptions {
					shared: cache.shared.unwrap_or(true),
					ignore_cargo_cult: true,
					..Default::default()
				}),
				..Default::default()
			};
			match store {
				CacheStore::Disk => {
					client = client.with(Cache(HttpCache {
						mode,
						manager: CACacheManager {
							path: cache
								.path
								.ok_or_else(|| {
									FaithError::new(
										FaithErrorKind::Config,
										Some("missing cache.path"),
									)
								})?
								.into(),
							remove_opts: Default::default(),
						},
						options: cache_options,
					}));
				}
				CacheStore::Memory => {
					client = client.with(Cache(HttpCache {
						mode,
						manager: MokaManager::new(
							MokaCacheBuilder::new(cache.capacity.map_or(10_000, |n| n.into()))
								.build(),
						),
						options: cache_options,
					}));
				}
			}
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
		{
			client = client.with(alt_svc_middleware);
		}

		Ok(Self {
			client: Some(client.build()),
			cookie_jar,
			stats: Default::default(),
			conn_tracker: ConnectionTracker::new(conn_timeout),
			#[cfg(feature = "http3")]
			alt_svc_cache,
			h3_follow_advertised_port,
		})
	}

	#[napi(constructor)]
	pub fn construct(env: Env, options: Option<AgentOptions>) -> Result<Self, napi::Error> {
		Ok(if let Some(options) = options {
			Self::with_options(options)
		} else {
			Self::new()
		}
		.map_err(|err| err.into_js_error(&env))?)
	}

	/// Close the agent, releasing its connection pool, DNS resolver, and any
	/// background tasks it owns, rather than waiting for the garbage collector
	/// to drop it. This is worth doing when you create many short-lived agents;
	/// a single long-lived agent can just be left to the GC.
	///
	/// Requests already in flight run to completion. Any new request on a closed
	/// agent throws a `Closed` error. Calling `close()` more than once is a
	/// no-op. The cookie store, if any, remains readable via `getCookie`.
	#[napi]
	pub fn close(&mut self) {
		// Dropping the client releases the reqwest connection pool and the
		// Hickory resolver task; the alt-svc cache goes with it.
		self.client = None;
		#[cfg(feature = "http3")]
		{
			self.alt_svc_cache = None;
		}
	}

	/// Add a cookie into the agent.
	///
	/// Does nothing if:
	/// - the cookie store is disabled
	/// - the url is malformed
	#[napi]
	pub fn add_cookie(&self, url: String, cookie: String) {
		let Some(jar) = &self.cookie_jar else {
			return;
		};

		let Ok(url) = Url::from_str(&url) else {
			return;
		};

		jar.add_cookie_str(&cookie, &url);
	}

	/// Retrieve a cookie from the store.
	///
	/// Returns `null` if:
	/// - there's no cookie at this url
	/// - the cookie store is disabled
	/// - the url is malformed
	/// - the cookie cannot be represented as a string
	#[napi]
	pub fn get_cookie(&self, url: String) -> Option<String> {
		let Some(jar) = &self.cookie_jar else {
			return None;
		};

		let Ok(url) = Url::from_str(&url) else {
			return None;
		};

		jar.cookies(&url)
			.and_then(|val| val.to_str().ok().map(ToOwned::to_owned))
	}

	/// Returns statistics gathered by this agent:
	///
	/// - `requestsSent`
	/// - `responsesReceived`
	/// - `bodiesStarted`
	/// - `bodiesFinished`
	#[napi]
	pub fn stats(&self) -> AgentStats {
		AgentStats {
			requests_sent: self
				.stats
				.requests_sent
				.load(Ordering::Relaxed)
				.try_into()
				.unwrap_or(i64::MAX),
			responses_received: self
				.stats
				.responses_received
				.load(Ordering::Relaxed)
				.try_into()
				.unwrap_or(i64::MAX),
			bodies_started: self
				.stats
				.bodies_started
				.load(Ordering::Relaxed)
				.try_into()
				.unwrap_or(i64::MAX),
			bodies_finished: self
				.stats
				.bodies_finished
				.load(Ordering::Relaxed)
				.try_into()
				.unwrap_or(i64::MAX),
		}
	}

	/// Returns information on current connections open by this agent.
	///
	/// Only tracks TCP connections currently (upstream limitation). Stats are updated once a second:
	/// this makes it possible to track indicators over time to find the retransmission rate, for
	/// example. The `lostPackets` and `deliveryRateBps` stats are only available on Linux. Some other
	/// fields might also be missing depending on platform support; and no forward guarantees are made
	/// on field availability. If the platform isn't supported at all, this will always return empty.
	#[napi]
	pub fn connections<'env>(&self, env: &'env Env) -> Vec<ConnectionInfo<'env>> {
		self.conn_tracker.get_for_napi(env)
	}
}
