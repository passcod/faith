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

use napi::bindgen_prelude::{PromiseRaw, within_runtime_if_available};

use http::Version;
use http_cache_reqwest::{
	CACacheManager, Cache, CacheOptions, HttpCache, HttpCacheOptions, MokaCacheBuilder, MokaManager,
};
use hyper_util::client::legacy::connect::HttpInfo;
use moka::sync::Cache as MokaCache;
use napi::{Either, Env, bindgen_prelude::Buffer};
use napi_derive::napi;
use reqwest::{
	Certificate, Client, Identity, Url,
	cookie::CookieStore as _,
	header::{HeaderMap, HeaderName, HeaderValue},
	redirect::Policy,
};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

#[cfg(feature = "http3")]
use crate::alt_svc::parse_alt_svc_header;
#[cfg(feature = "http3")]
use crate::alt_svc::{AltSvcCache, AltSvcCacheConfig, AltSvcMiddleware, H3Prober};
use crate::{
	async_task::faith_promise,
	conn_tracker::{ConnectionInfo, ConnectionTracker},
	cookies::{
		CookieLimits, DEFAULT_MAX_AGE, DEFAULT_MAX_PER_HOST, DEFAULT_MAX_SIZE, DEFAULT_MAX_TOTAL,
		FaithJar,
	},
	dns::FaithResolver,
	error::{FaithError, FaithErrorKind},
	options::{PRIORITY, RequestCacheMode},
	retry::DeadConnectionRetry,
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

/// Limits the cookie store enforces, from RFC 6265bis. Each is a cap; a caller who needs more room
/// raises the number.
///
/// The `__Host-` and `__Secure-` name prefix rules are what those prefixes mean, so they always
/// apply and are not settable here: a cookie that shouldn't carry them is named without one.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentCookieOptions {
	/// How far ahead of receipt a cookie may expire, in seconds. A cookie asking for longer, via
	/// `Max-Age` or `Expires`, has its expiry reduced to this; a shorter one is left alone and a
	/// session cookie stays a session cookie.
	///
	/// Default: 34_560_000 (400 days).
	pub max_age: Option<u32>,
	/// The largest cookie stored, as the combined length of its name and value in bytes. A larger
	/// cookie is not stored.
	///
	/// Default: 4096.
	pub max_size: Option<u32>,
	/// How many cookies are kept for any one domain, which is a cookie's `Domain` attribute when it
	/// has one and the host that set it otherwise.
	///
	/// Default: 180.
	pub max_per_host: Option<u32>,
	/// How many cookies are kept across the whole store, bounding a server that spreads cookies
	/// across subdomains to escape `maxPerHost`.
	///
	/// Default: 3000.
	pub max_total: Option<u32>,
}

impl From<&AgentCookieOptions> for CookieLimits {
	fn from(options: &AgentCookieOptions) -> Self {
		Self {
			max_age: options
				.max_age
				.map_or(DEFAULT_MAX_AGE, |secs| Duration::from_secs(secs.into())),
			max_size: options.max_size.map_or(DEFAULT_MAX_SIZE, |n| n as usize),
			max_per_host: options
				.max_per_host
				.map_or(DEFAULT_MAX_PER_HOST, |n| n as usize),
			max_total: options.max_total.map_or(DEFAULT_MAX_TOTAL, |n| n as usize),
		}
	}
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
	/// Use the system's DNS (via `getaddrinfo` or equivalent) rather than Faith's own DNS client (based on
	/// [Hickory]). If you experience issues with DNS where Faith does not work but e.g. curl or native
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
	/// but Faith (or rather its underlying QUIC library quinn, [does not implement those yet][2]).
	///
	/// [2]: https://github.com/quinn-rs/quinn/issues/1254
	///
	/// Default: `cubic`. Accepted values: `cubic`, `bbr1`.
	pub congestion: Option<Http3Congestion>,
	/// Maximum duration of inactivity to accept before timing out the connection, in seconds. Note that
	/// this only sets the timeout on this side of the connection: the true idle timeout is the _minimum_
	/// of this and the peer's own max idle timeout. While the underlying library has no limits, Faith
	/// defines bounds for safety: minimum 1 second, maximum 2 minutes (120 seconds).
	///
	/// Default: 30.
	pub max_idle_timeout: Option<u8>,
	/// Whether HTTP/3 upgrade via Alt-Svc is enabled. When enabled, the agent will track Alt-Svc
	/// headers from responses and automatically upgrade subsequent requests to HTTP/3 when available.
	///
	/// Default: true.
	pub upgrade_enabled: Option<bool>,
	/// Whether advertised HTTP/3 endpoints are verified with a background probe
	/// before any foreground request is routed to them.
	///
	/// An `Alt-Svc` advertisement says the server listens on UDP; it cannot say
	/// there is UDP connectivity between you and it. Without probing, the next
	/// request after an advertisement attempts HTTP/3 inline, and on a silently
	/// broken UDP path it stalls until the QUIC idle timeout or
	/// `upgradeAttemptTimeout` before falling back to TCP — recurring once per
	/// failure cooldown for as long as the path stays broken.
	///
	/// With probing (the default), requests keep using TCP until a background
	/// `HEAD /` over HTTP/3 has confirmed the path. The probe shares the
	/// connection pool, so the first upgraded request rides the probe's warm
	/// connection. A broken path costs one failed background request per
	/// cooldown and no foreground latency at all.
	///
	/// The probe is a synthetic request the server will see in its logs. Set
	/// this to `false` to restore the inline upgrade if that is unacceptable
	/// (per-request billing, easily-alarmed WAFs).
	///
	/// `hints` are exempt either way: a hint is your own assertion, so the first
	/// request to a hinted origin speaks HTTP/3 immediately, which is also what
	/// makes h3-only origins (no TCP listener) work.
	///
	/// Default: true.
	pub upgrade_probe: Option<bool>,
	/// Ceiling on how long a background HTTP/3 probe may take before the origin
	/// is treated as failed, in **milliseconds**.
	///
	/// This bounds background work only — no foreground request ever waits on a
	/// probe — so it can afford to be generous: a healthy handshake plus HEAD
	/// completes in one or two round trips. Set to 0 to leave probes bounded
	/// only by the QUIC idle timeout.
	///
	/// Default: 5000 (5 seconds).
	pub upgrade_probe_timeout: Option<u32>,
	/// Demote an origin off HTTP/3 when its QUIC path is provenly slower than
	/// its TCP path by this factor. Set to 0 to disable path-time demotion.
	///
	/// Faith keeps a per-origin moving average of time-to-response-headers for
	/// each protocol family. HTTP/3 is preferred at parity and when moderately
	/// slower — its advantages (no head-of-line blocking, connection migration)
	/// pay off beyond the average — so this factor should stay well above 1.
	/// Only a sustained gap acts: at least 8 samples on each side, and the QUIC
	/// average must also exceed the TCP one by an absolute 10ms so LAN-fast
	/// origins don't flap on noise.
	///
	/// A demoted origin is not treated as broken: it re-enters through a
	/// background probe after `upgradeSlowTtl`, asking whether the path has
	/// improved at zero foreground cost.
	///
	/// Default: 2.5.
	pub upgrade_slow_factor: Option<f64>,
	/// How long (in seconds) a path-time demotion holds before the origin is
	/// re-evaluated. See `upgradeSlowFactor`.
	///
	/// Default: 600 (10 minutes).
	pub upgrade_slow_ttl: Option<u32>,
	/// How long (in seconds) to cache an Alt-Svc advertisement before the first HTTP/3 attempt.
	/// This is overridden by the `ma` (max-age) parameter in the Alt-Svc header if present.
	///
	/// Default: 86400 (24 hours).
	pub upgrade_advertised_ttl: Option<u32>,
	/// How long (in seconds) to cache a confirmed working HTTP/3 connection.
	///
	/// Default: 86400 (24 hours).
	pub upgrade_confirmed_ttl: Option<u32>,
	/// How long (in seconds) a *first* failed HTTP/3 attempt blocks an origin. During this
	/// time, no HTTP/3 upgrades will be attempted for the origin, even if the server sends
	/// Alt-Svc headers.
	///
	/// Each consecutive failure doubles the cooldown, up to `upgradeFailedMaxTtl`, so an
	/// origin whose UDP path is blocked for good is retried less and less often instead of
	/// forever at this interval. A confirmed HTTP/3 response ends the run.
	///
	/// Default: 300 (5 minutes).
	pub upgrade_failed_ttl: Option<u32>,
	/// Ceiling (in seconds) on the cooldown that consecutive HTTP/3 failures double out of
	/// `upgradeFailedTtl`.
	///
	/// On the defaults an origin that keeps failing is blocked for 5 minutes, then 10, 20,
	/// 40, and an hour thereafter. Set this at or below `upgradeFailedTtl` for a flat
	/// cooldown that never backs off.
	///
	/// Default: 3600 (1 hour).
	pub upgrade_failed_max_ttl: Option<u32>,
	/// How many consecutive cancelled HTTP/3 attempts, within a 60-second window,
	/// demote an origin back to TCP.
	///
	/// Faith normally learns that HTTP/3 is broken from a failed attempt. A request
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
	/// So by default Faith does not upgrade at all when the advertised port
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
	/// Maximum bytes an origin may send on any one HTTP/3 stream before it must wait for
	/// Faith to acknowledge them. Overrides `flowControl.streamWindow` for HTTP/3 only.
	///
	/// Default: unset (`flowControl.streamWindow`, itself 6 MiB by default).
	pub stream_window: Option<u32>,
	/// Maximum bytes an origin may send across all streams of one HTTP/3 connection before it
	/// must wait for Faith to acknowledge them. Overrides `flowControl.connectionWindow` for
	/// HTTP/3 only.
	///
	/// Default: unset (`flowControl.connectionWindow`, itself 15 MiB by default).
	pub connection_window: Option<u32>,
	/// Maximum bytes Faith transmits to an origin without acknowledgement, bounding upload
	/// throughput the way the receive windows bound download. The origin's own flow control
	/// applies on top of this, so it is a ceiling rather than a grant.
	///
	/// This has no HTTP/2 counterpart: HTTP/2's send side is governed entirely by the window
	/// the peer advertises, with no local cap to set.
	///
	/// Default: 10 MB (quinn's own default).
	pub send_window: Option<u32>,
}

/// Settings related to HTTP/2. This is a nested object.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentHttp2Options {
	/// Maximum bytes an origin may send on any one HTTP/2 stream before it must wait for
	/// Faith to acknowledge them. Overrides `flowControl.streamWindow` for HTTP/2 only.
	///
	/// Ignored when `adaptiveWindow` is on.
	///
	/// Default: unset (`flowControl.streamWindow`, itself 6 MiB by default).
	pub stream_window: Option<u32>,
	/// Maximum bytes an origin may send across all streams of one HTTP/2 connection before it
	/// must wait for Faith to acknowledge them. Overrides `flowControl.connectionWindow` for
	/// HTTP/2 only.
	///
	/// Ignored when `adaptiveWindow` is on.
	///
	/// Default: unset (`flowControl.connectionWindow`, itself 15 MiB by default).
	pub connection_window: Option<u32>,
	/// Replace HTTP/2's static windows with windows that start small and grow towards a
	/// bandwidth-delay estimate sampled from connection pings, capped at 16 MiB.
	///
	/// This is off by default, and turning it on is usually the wrong move. A fresh connection
	/// opens at 64 KiB, 96 times below the static default, and doubles only when a ping sample
	/// reaches two thirds of the current estimate — so it takes many round trips to ramp up and
	/// carries *less* throughput than the static window for all but the largest transfers. It
	/// also takes over both windows, so `streamWindow` and `connectionWindow` stop applying.
	///
	/// Its one real advantage is memory: it holds a large window open only on connections that
	/// demonstrably need one. Since it caps at 16 MiB anyway, a static window near that ceiling
	/// buys the same throughput from the first byte.
	///
	/// HTTP/3 is unaffected either way, and keeps whichever windows apply to it.
	///
	/// Default: `false`.
	pub adaptive_window: Option<bool>,
}

/// Settings related to HTTP flow control, shared by HTTP/2 and HTTP/3. This is a nested object.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentFlowControlOptions {
	/// Maximum bytes an origin may send on any one stream before it must wait for Faith to
	/// acknowledge them, for HTTP/2 and HTTP/3 alike.
	///
	/// Larger windows keep a high-latency link full, at the cost of buffering more per stream.
	/// The default follows browser practice, and is deliberately at the conservative end of it:
	/// a pooled server-side client can hold many connections across many origins, so
	/// per-connection memory multiplies harder here than in a browser.
	///
	/// Set `http2.streamWindow` or `http3.streamWindow` to tune one protocol against the other.
	///
	/// Default: 6 MiB.
	pub stream_window: Option<u32>,
	/// Maximum bytes an origin may send across all streams of one connection before it must
	/// wait for Faith to acknowledge them, for HTTP/2 and HTTP/3 alike.
	///
	/// This is larger than `streamWindow` so concurrent streams on one connection share the
	/// connection's headroom, while still bounding the worst-case buffering of a connection
	/// carrying many concurrent requests.
	///
	/// Set `http2.connectionWindow` or `http3.connectionWindow` to tune one protocol against
	/// the other.
	///
	/// Default: 15 MiB.
	pub connection_window: Option<u32>,
}

/// Per-stream receive window applied to both protocols when nothing overrides it (spec:FLOW).
///
/// Chrome's shape: 6 MiB stream inside a 15 MiB connection. Picked over a larger window that
/// measured faster because it is what browsers have proven at scale, and because a pooled
/// server-side client multiplies per-connection memory across far more connections.
pub(crate) const DEFAULT_STREAM_WINDOW: u32 = 6 * 1024 * 1024;

/// Whole-connection receive window applied to both protocols when nothing overrides it (spec:FLOW).
pub(crate) const DEFAULT_CONNECTION_WINDOW: u32 = 15 * 1024 * 1024;

// Concurrent streams share the connection's headroom, so the asymmetry is the point of the
// defaults rather than an accident of the numbers (spec:FLOW#common-windows).
const _: () = assert!(DEFAULT_CONNECTION_WINDOW > DEFAULT_STREAM_WINDOW);

/// The flow-control windows to apply, once the common group, the per-protocol overrides, and the
/// defaults have been reconciled (spec:FLOW#per-protocol-windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedWindows {
	pub stream: u32,
	pub connection: u32,
}

/// Reconcile one protocol's windows: its own setting wins over the common one, which wins over the
/// default (spec:FLOW#per-protocol-windows).
pub(crate) fn resolve_windows(
	common: Option<&AgentFlowControlOptions>,
	protocol_stream: Option<u32>,
	protocol_connection: Option<u32>,
) -> ResolvedWindows {
	ResolvedWindows {
		stream: protocol_stream
			.or_else(|| common.and_then(|c| c.stream_window))
			.unwrap_or(DEFAULT_STREAM_WINDOW),
		connection: protocol_connection
			.or_else(|| common.and_then(|c| c.connection_window))
			.unwrap_or(DEFAULT_CONNECTION_WINDOW),
	}
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
/// - `follow`: automatically follow redirects. Faith limits this to 10 redirects.
/// - `error`: reject the promise with a network error when a redirect status is returned.
/// - ~~`manual`~~: not supported.
/// - `stop`: (Faith custom) don't follow any redirects, return the responses.
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
	/// `true` enables the store with the default limits; an options object enables it and tunes them,
	/// so `{}` means the same as `true`.
	///
	/// Default: `false`.
	///
	/// You may use `agent.getCookie(url: string)` and `agent.addCookie(url: string, value: string)` to add
	/// and retrieve cookies from the store.
	pub cookies: Option<Either<bool, AgentCookieOptions>>,
	/// Settings related to DNS. This is a nested object.
	pub dns: Option<AgentDnsOptions>,
	/// Flow-control windows shared by HTTP/2 and HTTP/3. This is a nested object.
	///
	/// Setting these is the normal way to tune windows: one value applies to whichever protocol
	/// a request negotiates, so throughput doesn't change when an origin upgrades from one to
	/// the other. The `http2` and `http3` groups override them per protocol.
	pub flow_control: Option<AgentFlowControlOptions>,
	/// Sets the default headers for every request.
	///
	/// If header names or values are invalid, they are silently omitted.
	/// Sensitive headers (e.g. `Authorization`) should be marked.
	///
	/// Default: none.
	pub headers: Option<Vec<Header>>,
	/// Settings related to HTTP/2. This is a nested object.
	pub http2: Option<AgentHttp2Options>,
	/// Settings related to HTTP/3. This is a nested object.
	pub http3: Option<AgentHttp3Options>,
	/// Bind outgoing sockets to this local IP address before connecting.
	///
	/// This also selects the address family of the HTTP/3 (QUIC) socket. By default that
	/// socket binds the IPv6 wildcard (`[::]`), which fails on hosts without usable IPv6 —
	/// there, HTTP/3 silently falls back to TCP. Faith detects that case automatically and
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

/// The `Agent` interface of the Faith API represents an instance of an HTTP client. Each `Agent` has
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
/// Faith use an `Agent`. For `fetch()` calls that don't specify one explicitly, a global agent with
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
	/// The raw `reqwest::Client` underlying [`Self::client`], sharing its connection pool. A
	/// `preconnect` warm-up sends its synthetic request here rather than through the middleware
	/// stack, which bypasses the HTTP cache and the Alt-Svc layer (and so keeps the warm-up out of
	/// request accounting), while still pooling the connection foreground requests reuse. `None`
	/// once the agent is closed. (spec:WARM)
	pub(crate) raw_client: Option<Client>,
	/// Faith's DNS resolver, shared with [`Self::client`] so `prefetchDns` warms the cache requests
	/// read. `None` under the system resolver, where there is no such cache. (spec:WARM)
	pub(crate) dns_resolver: Option<FaithResolver>,
	/// Origins with a warm-up connection opened within the pool idle window, so a repeat
	/// `preconnect` does no new work. Keyed by `scheme://host:port`; entries expire with the idle
	/// timeout. (spec:WARM)
	pub(crate) warmed: MokaCache<String, ()>,
	/// Single-flight claims for in-flight `preconnect` warm-ups, so concurrent calls for the same
	/// origin do not open duplicate connections. (spec:WARM)
	pub(crate) warming: MokaCache<String, ()>,
	pub(crate) cookie_jar: Option<Arc<FaithJar>>,
	pub(crate) stats: Arc<InnerAgentStats>,
	pub(crate) conn_tracker: Arc<ConnectionTracker>,
	#[cfg(feature = "http3")]
	#[allow(dead_code)]
	pub(crate) alt_svc_cache: Option<Arc<AltSvcCache>>,
	/// Held so `close()` can abort in-flight background probes: each one owns a
	/// clone of the raw client, which would otherwise keep the connection pool
	/// alive past close for up to the probe timeout.
	#[cfg(feature = "http3")]
	pub(crate) h3_prober: Option<Arc<H3Prober>>,
	/// Mirrors `http3.upgradeFollowAdvertisedPort`. Lives here because `fetch` needs
	/// it to stop a rewritten port from being reported as a redirect.
	pub(crate) h3_follow_advertised_port: bool,
	/// Mirrors `http3.upgradeEnabled`. A warm-up needs it to route the way a foreground request
	/// would: with the upgrade machinery off, nothing upgrades, whatever the caches hold.
	/// (spec:WARM#preconnect)
	#[cfg(feature = "http3")]
	pub(crate) h3_upgrade_enabled: bool,
	/// The agent's default `Accept-Encoding`, if one was set among its default headers.
	/// `fetch` consults it to decide which codings to decode when a request adds none of
	/// its own (see [`crate::encoding`]).
	pub(crate) default_accept_encoding: Option<HeaderValue>,
	/// Whether a `Priority` header sits among the agent's default headers. `fetch` consults
	/// it so that default wins over the header the `priority` option would derive.
	pub(crate) has_default_priority: bool,
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

		// `cookies: true` takes the default limits; an options object tunes them. (spec:COOK)
		let cookie_jar = match options.cookies.as_ref() {
			None | Some(Either::A(false)) => None,
			Some(Either::A(true)) => Some(Arc::new(FaithJar::new(CookieLimits::default()))),
			Some(Either::B(options)) => Some(Arc::new(FaithJar::new(options.into()))),
		};
		if let Some(jar) = &cookie_jar {
			client = client.cookie_provider(jar.clone());
		}

		let dns = options.dns.unwrap_or_default();
		let dns_resolver = if dns.system.unwrap_or(false) {
			// The system resolver (getaddrinfo) has no in-process cache Faith can warm, so no
			// resolver is installed and `prefetchDns` resolves as a no-op (spec:WARM).
			client = client.no_hickory_dns();
			None
		} else {
			for DnsOverride { domain, addresses } in dns.overrides.unwrap_or_default() {
				client = client.resolve_to_addrs(
					&domain,
					&addresses
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
						.collect::<Result<Vec<_>, FaithError>>()?,
				)
			}

			// Faith owns the hickory resolver rather than leaving it to reqwest's built-in one, so
			// `prefetchDns` can warm the very cache reqwest's requests read (spec:WARM). reqwest
			// still layers `dns.overrides` on top, so the overrides applied above keep working.
			let resolver = FaithResolver::default();
			client = client.dns_resolver(resolver.clone());
			Some(resolver)
		};

		let mut default_accept_encoding = None;
		let mut has_default_priority = false;
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
			default_accept_encoding = map.get(reqwest::header::ACCEPT_ENCODING).cloned();
			has_default_priority = map.contains_key(PRIORITY);
			client = client.default_headers(map);
		}

		// HTTP/2 flow control (spec:FLOW). Adaptive windowing takes over both windows itself, so
		// the explicit sizes are not applied at all when it's on: reqwest would let the later
		// `http2_adaptive_window` call win regardless, but leaving the calls out makes the
		// precedence visible here rather than depending on hyper's internal ordering.
		let http2 = options.http2.unwrap_or_default();
		if http2.adaptive_window.unwrap_or(false) {
			client = client.http2_adaptive_window(true);
		} else {
			let windows = resolve_windows(
				options.flow_control.as_ref(),
				http2.stream_window,
				http2.connection_window,
			);
			client = client
				.http2_initial_stream_window_size(windows.stream)
				.http2_initial_connection_window_size(windows.connection);
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

			// QUIC flow control (spec:FLOW). quinn's own defaults are a ~1.25MB stream window
			// inside an unbounded connection window: the stream window is the binding constraint
			// on a high-latency link, and the unbounded connection window means a connection with
			// many concurrent requests has no ceiling on what it buffers. Both are set here.
			let windows = resolve_windows(
				options.flow_control.as_ref(),
				options.http3.as_ref().and_then(|h| h.stream_window),
				options.http3.as_ref().and_then(|h| h.connection_window),
			);
			client = client
				.http3_stream_receive_window(windows.stream.into())
				.http3_conn_receive_window(windows.connection.into());

			if let Some(ref http3) = options.http3 {
				if let Some(Http3Congestion::Bbr1) = http3.congestion {
					client = client.http3_congestion_bbr();
				}

				if let Some(send_window) = http3.send_window {
					client = client.http3_send_window(send_window.into());
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
						// Hand reqwest the error unboxed: it boxes for us, and boxing first would
						// put a `Box<FaithError>` in the source chain, which does not downcast
						// back to `FaithError` when we come to recover the kind as a `code`.
						attempt.error(FaithError::from(FaithErrorKind::Redirect))
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
		let (alt_svc_cache, alt_svc_middleware, h3_prober, h3_upgrade_enabled) = {
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

			// The prober sends on the *raw* client, deliberately: it must skip
			// the HTTP cache (a replayed cached response would fake a
			// confirmation) and this very middleware (no recursion), while
			// sharing the h3 connection pool so a successful probe leaves a warm
			// connection for the foreground. Only built when both the upgrade
			// machinery and probing are on.
			let prober = (enabled && probe).then(|| {
				Arc::new(H3Prober::new(
					reqwest_client.clone(),
					cache.clone(),
					probe_timeout,
				))
			});

			let middleware =
				AltSvcMiddleware::new(cache.clone(), enabled, attempt_timeout, prober.clone());

			// Registered below rather than here — see the note at the registration.
			(Some(cache), middleware, prober, enabled)
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

		// Registered last, so it sits innermost and wraps nothing but the exchange
		// itself. Inside the Alt-Svc layer rather than outside it, because each
		// protocol attempt is its own connection and deserves its own retry: a
		// failed HTTP/3 attempt is the fallback's business, and re-running the
		// upgrade decision from out here would re-attempt HTTP/3 on a path already
		// judged dead and record a second failure against the origin for it. Inside
		// the HTTP cache for the same reason as the Alt-Svc layer -- a retry should
		// re-send the request, not redo the cache lookup that led to it.
		client = client.with(DeadConnectionRetry);

		Ok(Self {
			client: Some(client.build()),
			raw_client: Some(reqwest_client),
			dns_resolver,
			// A warm-up connection is warm only as long as the pool keeps it idle, so the record
			// that an origin is warm expires with that same window.
			warmed: MokaCache::builder().time_to_live(conn_timeout).build(),
			// A safety TTL well past any reasonable warm-up, so a claim that never gets released
			// (a warm-up whose task is dropped) frees the origin rather than wedging it.
			warming: MokaCache::builder()
				.time_to_live(Duration::from_secs(300))
				.build(),
			cookie_jar,
			stats: Default::default(),
			conn_tracker: ConnectionTracker::new(conn_timeout),
			#[cfg(feature = "http3")]
			alt_svc_cache,
			#[cfg(feature = "http3")]
			h3_prober,
			h3_follow_advertised_port,
			#[cfg(feature = "http3")]
			h3_upgrade_enabled,
			default_accept_encoding,
			has_default_priority,
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

	/// Add a cookie into the agent.
	///
	/// The cookie goes through the same rules a `Set-Cookie` header would, with the url supplying
	/// the scheme and host they read, so this does nothing if:
	/// - the cookie store is disabled
	/// - the url is malformed
	/// - the cookie does not parse
	/// - a `__Host-` or `__Secure-` name prefix is not satisfied
	/// - the cookie is larger than `cookies.maxSize`
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

	/// Note that a request reached this origin, so it holds a connection the pool keeps idle for
	/// the idle window and a `preconnect` for it has no new work to do (spec:WARM).
	///
	/// Called for foreground requests as well as warm-ups, because the criterion is about the
	/// origin holding an idle pooled connection, not about how it came to hold one.
	pub(crate) fn mark_warm(&self, url: &Url) {
		self.warmed.insert(origin_key(url), ());
	}

	/// Warm the DNS cache for `host`, so a later request to it skips the lookup.
	///
	/// Mirrors the browser's `dns-prefetch` resource hint. The argument is a bare host; a scheme,
	/// port, or path in a fuller string is ignored. The returned promise resolves when the answer
	/// lands in the cache and never rejects, whatever happens on the network — a resolution failure
	/// resolves quietly, because the work is advisory. Under the system resolver there is no cache
	/// to warm, so the call resolves without doing anything. A malformed host throws synchronously,
	/// as does a call on a closed agent. (spec:WARM)
	#[napi]
	pub fn prefetch_dns<'env>(
		&self,
		env: &'env Env,
		host: String,
	) -> Result<PromiseRaw<'env, ()>, napi::Error> {
		if self.client.is_none() {
			return Err(caller_error(env, FaithErrorKind::Closed));
		}

		let Some(host) = extract_host(&host) else {
			return Err(caller_error(env, FaithErrorKind::AddressParse));
		};

		let resolver = self.dns_resolver.clone();
		faith_promise(env, async move {
			if let Some(resolver) = resolver {
				resolver.prefetch(&host).await;
			}
			Ok(())
		})
	}

	/// Open a pooled connection to `origin`, so the first request to it skips DNS, TCP, and TLS
	/// setup.
	///
	/// Mirrors the browser's `preconnect` resource hint. The argument is an origin
	/// (`scheme://host[:port]`); a longer URL is reduced to its origin. The warm-up sends a
	/// synthetic `HEAD` to the origin's root — the origin sees it — over the transport the next
	/// foreground request would use: a confirmed HTTP/3 origin gets a warm QUIC connection, every
	/// other origin a TCP one. The returned promise resolves when the attempt finishes and never
	/// rejects: every network failure resolves quietly. A malformed origin throws synchronously, as
	/// does a call on a closed agent. (spec:WARM)
	#[napi]
	pub fn preconnect<'env>(
		&self,
		env: &'env Env,
		origin: String,
	) -> Result<PromiseRaw<'env, ()>, napi::Error> {
		let Some(raw_client) = self.raw_client.clone() else {
			return Err(caller_error(env, FaithErrorKind::Closed));
		};

		let Some(url) = reduce_to_origin(&origin) else {
			return Err(caller_error(env, FaithErrorKind::AddressParse));
		};
		let key = origin_key(&url);

		// Already warm within the idle window, or a warm-up for this origin already in flight:
		// either way there is no new work to do, so resolve without opening a duplicate.
		if self.warmed.contains_key(&key)
			|| !self.warming.entry(key.clone()).or_insert(()).is_fresh()
		{
			return faith_promise(env, async move { Ok(()) });
		}

		// The transport the next foreground request would take, decided exactly as
		// `AltSvcMiddleware` decides it: nothing upgrades with the machinery off; with a prober,
		// only a confirmed origin routes to QUIC (an advertisement is evidence worth probing, not
		// worth routing on); without one, the legacy inline upgrade acts on advertisements too.
		// Diverging here would warm the wrong transport (spec:WARM#preconnect).
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

		#[cfg(feature = "http3")]
		let alt_svc_cache = self.alt_svc_cache.clone();
		#[cfg(feature = "http3")]
		let h3_prober = self.h3_prober.clone();

		let conn_tracker = self.conn_tracker.clone();
		let warmed = self.warmed.clone();
		let warming = self.warming.clone();

		faith_promise(env, async move {
			// Release the single-flight claim whatever happens, so a later warm-up isn't blocked
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
					// A port differing from the origin's only comes back with
					// `upgradeFollowAdvertisedPort` on; rewriting the URL is how reqwest is told to
					// connect there (mirrors the foreground path).
					if Some(port) != h3_url.port_or_known_default() {
						let _ = h3_url.set_port(Some(port));
					}
					raw_client.head(h3_url).version(Version::HTTP_3)
				}
				None => raw_client.head(url.clone()),
			};

			let outcome = request.send().await;

			// A TCP warm-up leaves a pooled connection to track and, in probe mode, may reveal an
			// HTTP/3 advertisement to act on; a QUIC warm-up does neither (QUIC connections are not
			// tracked, and a confirmed origin has nothing left to probe).
			if h3_port.is_none() {
				if let Ok(response) = &outcome
					&& let Some(info) = response.extensions().get::<HttpInfo>()
				{
					conn_tracker.track_warmup(info.local_addr(), info.remote_addr());
				}

				// A background probe verifies HTTP/3 for a probe-worthy origin exactly as a real
				// TCP-routed request would, folding in any fresh advertisement first; the warm-up
				// settles without waiting for it. Both only apply in probe mode (a prober present).
				#[cfg(feature = "http3")]
				if let Some(prober) = &h3_prober {
					if let (Some(cache), Ok(response)) = (&alt_svc_cache, &outcome)
						&& let Some(value) = response.headers().get("alt-svc")
						&& let Ok(value) = value.to_str()
						&& let Some(advertisement) = parse_alt_svc_header(value)
					{
						cache.record_alt_svc(&url, &advertisement);
					}
					prober.maybe_probe(&url);
				}
			}

			// A connection was established, so the origin is warm for the idle window; a failed
			// warm-up leaves it unmarked so a later `preconnect` may try again.
			if outcome.is_ok() {
				warmed.insert(key, ());
			}

			Ok(())
		})
	}
}

/// Build the JS error a warm-up throws synchronously for a caller mistake, preserving its `.code`
/// and JS error class. Network failures never reach here — they resolve quietly (spec:WARM).
fn caller_error(env: &Env, kind: FaithErrorKind) -> napi::Error {
	napi::Error::from(FaithError::from(kind).into_js_error(env))
}

/// Extract the bare host from a `prefetchDns` argument, ignoring any scheme, port, or path, or
/// `None` if there is no host to resolve. A DNS name carries none of those parts, so a fuller
/// string is reduced to its host. (spec:WARM)
fn extract_host(input: &str) -> Option<String> {
	// A string that already spells a scheme is read as the URL it is; anything else is the
	// bare-host case, where a name is not a URL on its own and giving it an authority makes it
	// parse as one. Telling the two apart on the scheme separator matters both ways: `example.com:8443`
	// otherwise parses as a *scheme* of `example.com` with no host, and a schemed string with no host
	// (`file:///path`, a bare `https://`) would have its scheme misread as a host by the fallback.
	let url = if input.contains("://") {
		Url::parse(input).ok()?
	} else {
		Url::parse(&format!("dns://{input}")).ok()?
	};
	let host = url.host_str()?;
	// `host_str` brackets an IPv6 literal; the resolver wants it bare.
	let host = host
		.strip_prefix('[')
		.and_then(|rest| rest.strip_suffix(']'))
		.unwrap_or(host);
	(!host.is_empty()).then(|| host.to_owned())
}

/// Reduce a `preconnect` argument to its origin, or `None` if it is not a connectable origin. Path,
/// query, fragment, and userinfo are stripped — the same reduction the HTTP/3 probe applies — and
/// the scheme must have a known default port so an omitted port resolves. (spec:WARM)
fn reduce_to_origin(input: &str) -> Option<Url> {
	let mut url = Url::parse(input).ok()?;
	if !url.has_host() || url.port_or_known_default().is_none() {
		return None;
	}
	url.set_path("/");
	url.set_query(None);
	url.set_fragment(None);
	let _ = url.set_username("");
	let _ = url.set_password(None);
	Some(url)
}

/// The `scheme://host:port` key an origin coalesces on, with the port defaulted by scheme so
/// `https://host` and `https://host:443` are the same origin. Matches the Alt-Svc cache's key.
pub(crate) fn origin_key(url: &Url) -> String {
	format!(
		"{}://{}:{}",
		url.scheme(),
		url.host_str().unwrap_or_default(),
		url.port_or_known_default().unwrap_or_default(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn prefetch_dns_takes_a_bare_host() {
		assert_eq!(extract_host("example.com").as_deref(), Some("example.com"));
	}

	#[test]
	fn prefetch_dns_ignores_the_parts_a_name_does_not_have() {
		// A DNS name has no scheme, port, or path, so a fuller string is reduced to its host
		// rather than rejected (spec:WARM#prefetchdns).
		for input in [
			"https://example.com",
			"https://example.com:8443",
			"https://example.com/some/path?q=1#frag",
			"https://user:pass@example.com/",
			"example.com:8443",
		] {
			assert_eq!(
				extract_host(input).as_deref(),
				Some("example.com"),
				"{input:?} names example.com whatever else it carries"
			);
		}
	}

	#[test]
	fn prefetch_dns_unwraps_an_ipv6_literal() {
		// `host_str` brackets an IPv6 literal, but the resolver wants it bare.
		assert_eq!(
			extract_host("https://[2001:db8::1]:8443").as_deref(),
			Some("2001:db8::1")
		);
	}

	#[test]
	fn prefetch_dns_rejects_a_string_with_no_host() {
		for input in ["", "   ", "/just/a/path", "https://"] {
			assert!(
				extract_host(input).is_none(),
				"{input:?} names no host to resolve"
			);
		}
	}

	#[test]
	fn preconnect_reduces_a_longer_url_to_its_origin() {
		// The same reduction the HTTP/3 probe applies (spec:WARM#preconnect).
		let url = reduce_to_origin("https://user:pass@example.com/some/path?q=1#frag")
			.expect("a full URL reduces to its origin");

		assert_eq!(url.as_str(), "https://example.com/");
		assert_eq!(url.username(), "", "userinfo is stripped");
		assert_eq!(url.password(), None);
		assert_eq!(url.query(), None);
		assert_eq!(url.fragment(), None);
	}

	#[test]
	fn preconnect_defaults_the_port_by_scheme() {
		// An omitted port defaults by scheme, so an origin spelled either way coalesces on one
		// key (spec:WARM#preconnect).
		for (bare, spelled) in [
			("https://example.com", "https://example.com:443"),
			("http://example.com", "http://example.com:80"),
		] {
			let bare = origin_key(&reduce_to_origin(bare).expect("parses"));
			let spelled = origin_key(&reduce_to_origin(spelled).expect("parses"));
			assert_eq!(
				bare, spelled,
				"the omitted port defaults to the spelled one"
			);
		}
	}

	#[test]
	fn preconnect_keeps_distinct_origins_apart() {
		// The pool caps and the warm record are per origin: scheme, host, and port together
		// (spec:POOL).
		let key = |input: &str| origin_key(&reduce_to_origin(input).expect("parses"));

		assert_ne!(key("https://example.com"), key("https://example.com:8443"));
		assert_ne!(key("https://example.com"), key("http://example.com"));
		assert_ne!(key("https://example.com"), key("https://other.example"));
	}

	#[test]
	fn preconnect_rejects_what_cannot_be_connected_to() {
		for input in [
			"not an origin",
			"",
			"/just/a/path",
			// No host to connect to.
			"file:///etc/hosts",
			// No default port for the scheme, and none given.
			"unknownscheme://example.com",
		] {
			assert!(
				reduce_to_origin(input).is_none(),
				"{input:?} is not a connectable origin"
			);
		}
	}

	fn common(stream: Option<u32>, connection: Option<u32>) -> AgentFlowControlOptions {
		AgentFlowControlOptions {
			stream_window: stream,
			connection_window: connection,
		}
	}

	#[test]
	fn windows_fall_back_to_the_defaults() {
		// An agent configured with nothing at all still gets the large static windows
		// (spec:FLOW#common-windows).
		assert_eq!(
			resolve_windows(None, None, None),
			ResolvedWindows {
				stream: 6 * 1024 * 1024,
				connection: 15 * 1024 * 1024,
			}
		);
	}

	#[test]
	fn the_common_windows_apply_when_a_protocol_says_nothing() {
		assert_eq!(
			resolve_windows(Some(&common(Some(1024), Some(4096))), None, None),
			ResolvedWindows {
				stream: 1024,
				connection: 4096,
			}
		);
	}

	#[test]
	fn a_protocol_window_beats_the_common_one() {
		// The whole point of the per-protocol group: tune one protocol against the other
		// (spec:FLOW#per-protocol-windows).
		assert_eq!(
			resolve_windows(
				Some(&common(Some(1024), Some(4096))),
				Some(2048),
				Some(8192)
			),
			ResolvedWindows {
				stream: 2048,
				connection: 8192,
			}
		);
	}

	#[test]
	fn each_window_falls_back_on_its_own() {
		// Overriding the stream window for one protocol leaves that protocol's connection
		// window on the common value, rather than dropping it to the default.
		assert_eq!(
			resolve_windows(Some(&common(Some(1024), Some(4096))), Some(2048), None),
			ResolvedWindows {
				stream: 2048,
				connection: 4096,
			}
		);
		assert_eq!(
			resolve_windows(Some(&common(None, None)), None, Some(8192)),
			ResolvedWindows {
				stream: DEFAULT_STREAM_WINDOW,
				connection: 8192,
			}
		);
	}

	#[test]
	fn a_protocol_window_applies_without_the_common_group() {
		assert_eq!(
			resolve_windows(None, Some(2048), None),
			ResolvedWindows {
				stream: 2048,
				connection: DEFAULT_CONNECTION_WINDOW,
			}
		);
	}

	#[test]
	fn the_two_protocols_resolve_independently() {
		// One `flowControl` value covers both protocols, and overriding it for HTTP/3 leaves
		// HTTP/2 where it was (spec:FLOW#per-protocol-windows).
		let flow = common(Some(1024), Some(4096));
		let http2 = resolve_windows(Some(&flow), None, None);
		let http3 = resolve_windows(Some(&flow), Some(2048), None);

		assert_eq!(http2.stream, 1024);
		assert_eq!(http3.stream, 2048);
		assert_eq!(http2.connection, http3.connection);
	}
}
