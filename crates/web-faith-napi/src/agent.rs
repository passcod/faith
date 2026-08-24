use std::{
	fmt::Debug,
	net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket},
	str::FromStr as _,
	sync::Arc,
	time::Duration,
};

use napi::bindgen_prelude::{PromiseRaw, within_runtime_if_available};

use http_cache_reqwest::{
	CACacheManager, CacheOptions, HttpCacheOptions, MokaCacheBuilder, MokaManager,
};
use napi::{Either, Env, bindgen_prelude::Buffer};
use napi_derive::napi;
use reqwest::{
	Certificate, Identity, Url,
	header::{HeaderMap, HeaderName, HeaderValue},
};

use web_faith::agent::AgentSettings;
#[cfg(feature = "http3")]
use web_faith::client::H3UpgradeRecipe;
use web_faith::client::{
	ClientRecipe, DEFAULT_CONNECTION_WINDOW, DEFAULT_STREAM_WINDOW, HttpCacheRecipe,
	HttpCacheStore, NodeEnvRecipe, RedirectPolicy, ResolvedWindows,
};
#[cfg(feature = "http3")]
use web_faith_alt_svc::{AltSvcCache, AltSvcCacheConfig};
use web_faith_dns::{
	DEFAULT_MAX_STALE, FaithResolver, ResolverSettings, ServerSpec, parse_domains,
};

use web_faith_cookies::{
	CookieLimits, DEFAULT_MAX_AGE, DEFAULT_MAX_PER_HOST, DEFAULT_MAX_SIZE, DEFAULT_MAX_TOTAL,
	FaithJar,
};

use crate::{
	async_task::faith_promise,
	conn_tracker::{ConnectionInfo, connections_for_napi},
	error::{FaithError, FaithErrorExt, FaithErrorKind},
	options::{PRIORITY, RequestCacheMode},
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
	/// An ordered list of resolver URLs, each URL's scheme selecting the transport Faith speaks to
	/// that resolver: `udp://` and `tcp://` for conventional DNS on port 53, `tls://` for DNS over
	/// TLS on port 853, `https://` for DNS over HTTPS on port 443, `quic://` for DNS over QUIC on
	/// port 853, and `h3://` for DNS over HTTP/3 on port 443. A port in the URL overrides the
	/// conventional one, and the HTTP transports use `/dns-query` when the URL supplies no path.
	///
	/// The encrypted transports always authenticate the resolver. A URL fragment names the
	/// certificate to expect (`tls://1.1.1.1#cloudflare-dns.com`); a hostname host authenticates
	/// against the hostname; a bare-IP host authenticates against the address itself.
	///
	/// Servers are queried in order, a later one reached only once those before it fail. Setting
	/// this replaces the system's servers, so no discovery runs. Throws if a URL is unparseable or
	/// its scheme is not one of the above, and combining it with `dns.system` throws.
	///
	/// Default: system discovery.
	pub servers: Option<Vec<String>>,
	/// Bound name resolution across the whole server list, in milliseconds. Exhausting several dead
	/// servers costs a single timeout rather than one per server.
	///
	/// Default: 5000.
	pub timeout: Option<u32>,
	/// Replace the system's search list, the domains appended to a name that is not fully
	/// qualified. Independent of `dns.servers`.
	///
	/// Default: the system's search list.
	pub search_domains: Option<Vec<String>>,
	/// How many dots a name must contain before it is tried as given, ahead of the search list.
	/// Independent of `dns.servers`.
	///
	/// Default: the system's setting.
	pub ndots: Option<u32>,
	/// Turn hosts-file lookup on or off. When unset, follows the platform's own convention.
	///
	/// Default: platform convention.
	pub hosts_file: Option<bool>,
	/// Further domains to exempt from the configured or encrypted resolver, for the internal
	/// suffixes a network uses. Added to the always-exempt `localhost`, `.local`, and the network's
	/// own DNS suffix; a domain is exempt when it matches an entry exactly or is a subdomain of one.
	///
	/// Default: no extra exemptions.
	pub exempt_domains: Option<Vec<String>>,
	/// Serve an expired cache entry immediately and refresh it in the background, rather than making
	/// the lookup wait for a fresh answer. A host's address changes rarely, so an expired answer is
	/// almost always still correct, and a connect failure against one that has moved re-resolves and
	/// attempts the request again.
	///
	/// Set `false` for an agent that must never connect to an address it knows to be out of date: an
	/// expired entry is discarded and the lookup blocks on a fresh answer.
	///
	/// Default: true.
	pub serve_stale: Option<bool>,
	/// How far past expiry an answer may still be served, in milliseconds. An entry older than this
	/// is discarded rather than served: an answer stale enough stops being evidence about where the
	/// host is, and a refresh still failing after that long is the case where the address most likely
	/// did change.
	///
	/// Default: 3600000 (one hour).
	pub max_stale: Option<u32>,
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

/// Switches that depart from standard behaviour on purpose. This is a nested object.
///
/// Each quirk turns off a rule Faith otherwise upholds, in exchange for a capability the rule
/// forbids. All of them are off by default, so an agent constructed with no options is
/// standards-compliant. A quirk is for a caller who controls the origin, or has otherwise
/// established that what the rule guards against does not apply to them: turning one on means
/// requests may fail against origins that expect the standard behaviour.
#[napi(object)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentQuirksOptions {
	/// Allow a streaming request body to be sent over an HTTP/1.x connection.
	///
	/// The fetch standard reserves streaming request bodies for HTTP/2 and HTTP/3: a body read
	/// from a `ReadableStream` has no known length when the headers go out, and an HTTP/1.x
	/// origin or an intermediary on the path may refuse it. With this on, such a body sends over
	/// whichever protocol the connection negotiates.
	///
	/// Default: false.
	pub h1_request_streaming: Option<bool>,
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

/// `manual` is not supported and behaves as `follow`, which is what the client's own policy spells
/// out: it carries no variant for a choice that never differed.
impl From<Redirect> for RedirectPolicy {
	fn from(redirect: Redirect) -> Self {
		match redirect {
			Redirect::Follow | Redirect::Manual => Self::Follow,
			Redirect::Error => Self::Error,
			Redirect::Stop => Self::Stop,
		}
	}
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
	/// Switches that depart from standard behaviour on purpose. This is a nested object.
	pub quirks: Option<AgentQuirksOptions>,
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

impl From<web_faith::stats::AgentStats> for AgentStats {
	fn from(stats: web_faith::stats::AgentStats) -> Self {
		let count = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
		Self {
			requests_sent: count(stats.requests_sent),
			responses_received: count(stats.responses_received),
			bodies_started: count(stats.bodies_started),
			bodies_finished: count(stats.bodies_finished),
		}
	}
}

/// One entry of `Agent.resolvers()`: a DNS server the agent resolves through (spec:OBS#resolvers).
#[napi(object)]
#[derive(Debug, Clone)]
pub struct ResolverInfo {
	/// The server's address, as `ip:port`.
	pub address: String,
	/// The transport in use: `udp`, `tcp`, `tls`, `https`, `quic`, or `h3`.
	pub transport: String,
	/// How the transport was arrived at: `configured` by the caller, or `conventional` DNS.
	pub source: String,
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
	pub(crate) inner: web_faith::agent::Agent,
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
		// Destructured rather than read field by field so that a new option cannot be added
		// without the compiler pointing here, where every option is turned into the recipe the
		// agent's clients are built from (spec:NETCHG).
		let AgentOptions {
			cache,
			cookies,
			dns,
			flow_control,
			headers,
			http2,
			// Every use of the HTTP/3 options sits behind the feature.
			#[cfg_attr(not(feature = "http3"), allow(unused_variables))]
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
		let local_address = match &local_address {
			Some(addr) => Some(IpAddr::from_str(addr).map_err(|err| {
				FaithError::new(
					FaithErrorKind::AddressParse,
					Some(format!("{addr:?}: {err}")),
				)
			})?),
			None if !ipv6_wildcard_bindable() => Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
			None => None,
		};

		// `cookies: true` takes the default limits; an options object tunes them. (spec:COOK)
		// The jar is installed on the client by the recipe, so it survives a rebuild
		// (spec:NETCHG#what-the-signal-keeps).
		let cookie_jar = match cookies.as_ref() {
			None | Some(Either::A(false)) => None,
			Some(Either::A(true)) => Some(Arc::new(FaithJar::new(CookieLimits::default()))),
			Some(Either::B(cookies)) => Some(Arc::new(FaithJar::new(cookies.into()))),
		};

		let dns = dns.unwrap_or_default();
		let dns_system = dns.system.unwrap_or(false);
		// Naming servers and asking for the system resolver at once is a contradiction rather than
		// a preference, since the system resolver is not Faith's to point at listed servers
		// (spec:DNS#system-resolver).
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

		let mut default_accept_encoding = None;
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
			default_accept_encoding = map.get(reqwest::header::ACCEPT_ENCODING).cloned();
			default_content_encoding = map.get(reqwest::header::CONTENT_ENCODING).cloned();
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
					Some(identity) => Some(
						Identity::from_pem(match identity {
							Either::A(buf) => buf.as_ref(),
							Either::B(string) => string.as_bytes(),
						})
						.map_err(|err| {
							FaithError::new(FaithErrorKind::PemParse, Some(err.to_string()))
						})?,
					),
				};

				let mut extra_roots = Vec::new();
				for pem in tls.extra_roots.iter().flatten() {
					let bytes = match pem {
						Either::A(buf) => buf.as_ref(),
						Either::B(string) => string.as_bytes(),
					};
					extra_roots.extend(Certificate::from_pem_bundle(bytes).map_err(|err| {
						FaithError::new(FaithErrorKind::PemParse, Some(err.to_string()))
					})?);
				}

				(identity, extra_roots)
			}
		};

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
			redirect: redirect.map(RedirectPolicy::from),
			connect_timeout,
			read_timeout,
			total_timeout,
			#[cfg(feature = "http3")]
			tls_early_data,
			tls_identity,
			tls_required,
			tls_extra_roots,
			node_env: NodeEnvRecipe::read(),
			http_cache,
			#[cfg(feature = "http3")]
			h3_upgrade,
		};

		let settings = AgentSettings {
			h3_follow_advertised_port,
			#[cfg(feature = "http3")]
			h3_upgrade_enabled: recipe.h3_upgrade.enabled,
			quirk_h1_request_streaming,
			default_accept_encoding,
			default_content_encoding,
			has_default_priority,
		};

		Ok(Self {
			inner: web_faith::agent::Agent::build(
				recipe,
				settings,
				cookie_jar,
				dns_resolver,
				#[cfg(feature = "http3")]
				alt_svc_cache,
			)?,
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
		self.inner.close();
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
	#[napi]
	pub fn network_changed(&mut self) {
		self.inner.network_changed();
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
		let Ok(url) = Url::from_str(&url) else {
			return;
		};

		self.inner.add_cookie(&url, &cookie);
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
		let url = Url::from_str(&url).ok()?;
		self.inner.cookie_header(&url)
	}

	/// Returns statistics gathered by this agent:
	///
	/// - `requestsSent`
	/// - `responsesReceived`
	/// - `bodiesStarted`
	/// - `bodiesFinished`
	#[napi]
	pub fn stats(&self) -> AgentStats {
		AgentStats::from(self.inner.stats())
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
		connections_for_napi(&self.inner.conn_tracker, env)
	}

	/// Returns the DNS servers this agent resolves through, in the order they are queried, so
	/// "are my lookups actually encrypted" is answerable from inside the process.
	///
	/// Each entry gives the server's address, the transport in use (`udp`, `tcp`, `tls`, `https`,
	/// `quic`, or `h3`), and how that transport was arrived at (`configured` or `conventional`).
	/// The list is empty until the resolver has been used, because it reads its configuration on
	/// first use, and empty for an agent using the system resolver.
	#[napi]
	pub fn resolvers(&self) -> Vec<ResolverInfo> {
		self.inner
			.resolvers()
			.into_iter()
			.map(|report| ResolverInfo {
				address: report.address,
				transport: report.transport,
				source: report.source,
			})
			.collect()
	}

	/// Warm the DNS cache for `host`, so a later request to it skips the lookup.
	///
	/// Mirrors the browser's `dns-prefetch` resource hint. The argument is a bare host; a scheme,
	/// port, or path in a fuller string is ignored. The returned promise resolves when the answer
	/// lands in the cache and never rejects, whatever happens on the network — a resolution failure
	/// resolves quietly, because the work is advisory. Under the system resolver there is no cache
	/// to warm, so the call resolves without doing anything. A malformed host throws synchronously,
	/// as does a call on a closed agent.
	#[napi]
	pub fn prefetch_dns<'env>(
		&self,
		env: &'env Env,
		host: String,
	) -> Result<PromiseRaw<'env, ()>, napi::Error> {
		let warming = self
			.inner
			.prefetch_dns(&host)
			.map_err(|err| caller_error(env, err))?;
		faith_promise(env, async move {
			warming.await;
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
	/// does a call on a closed agent.
	#[napi]
	pub fn preconnect<'env>(
		&self,
		env: &'env Env,
		origin: String,
	) -> Result<PromiseRaw<'env, ()>, napi::Error> {
		let warming = self
			.inner
			.preconnect(&origin)
			.map_err(|err| caller_error(env, err))?;
		faith_promise(env, async move {
			warming.await;
			Ok(())
		})
	}
}

/// Build the JS error a warm-up throws synchronously for a caller mistake, preserving its `.code`
/// and JS error class. Network failures never reach here — they resolve quietly (spec:WARM).
fn caller_error(env: &Env, err: FaithError) -> napi::Error {
	napi::Error::from(err.into_js_error(env))
}

#[cfg(test)]
mod tests {
	use super::*;

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
