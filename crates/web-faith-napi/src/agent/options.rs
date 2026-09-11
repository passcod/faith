//! The `AgentOptions` object as JavaScript spells it, and the option groups under it.

use std::fmt::Debug;

use napi::{Either, bindgen_prelude::Buffer};
use napi_derive::napi;

#[cfg(feature = "cookies")]
use std::time::Duration;
use web_faith::options::RedirectPolicy;

#[cfg(feature = "cookies")]
use web_faith_cookies::{
	CookieLimits, DEFAULT_MAX_AGE, DEFAULT_MAX_PER_HOST, DEFAULT_MAX_SIZE, DEFAULT_MAX_TOTAL,
};

use crate::options::RequestCacheMode;

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

#[cfg(feature = "cookies")]
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
