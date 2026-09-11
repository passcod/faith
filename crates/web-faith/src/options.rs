//! Agent options.
//!
//! Typically reached through [`Agent::builder`](crate::Agent::builder) rather than named directly.

// spec:AGENT

use std::net::{IpAddr, Ipv6Addr, SocketAddr, UdpSocket};

#[cfg(feature = "cache")]
use http_cache_reqwest::CacheMode;

#[cfg(feature = "cookies")]
use web_faith_cookies::CookieLimits;

use crate::client::{DEFAULT_CONNECTION_WINDOW, DEFAULT_STREAM_WINDOW, ResolvedWindows};

/// Milliseconds, saturating rather than wrapping on a duration no setting could mean.
fn millis(duration: std::time::Duration) -> u32 {
	duration.as_millis().try_into().unwrap_or(u32::MAX)
}

/// Whole seconds, rounded down, saturating as above.
fn secs(duration: std::time::Duration) -> u32 {
	duration.as_secs().try_into().unwrap_or(u32::MAX)
}

/// How to handle a redirect response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RedirectPolicy {
	/// Follow redirects, up to the standard's limit.
	#[default]
	Follow,
	/// Refuse the redirect and report it as an error.
	Error,
	/// Return the redirect response itself.
	Stop,
}

/// HTTP cache settings.
#[cfg(feature = "cache")]
#[derive(bon::Builder, Clone, Debug, Default)]
#[non_exhaustive]
pub struct CacheOptions {
	/// Which cache store to use: either `disk` or `memory`.
	///
	/// Default: none (cache disabled).
	pub store: Option<CacheStore>,
	/// If `cache.store: "memory"`, the maximum amount of items stored.
	///
	/// Default: 10_000.
	pub capacity: Option<u32>,
	/// Default cache mode, used when a request sets none of its own.
	///
	/// Default: [`CacheMode::Default`].
	pub mode: Option<CacheMode>,
	/// If `cache.store: "disk"`, then this is the path at which the cache data is. Must be writeable.
	///
	/// Required if `cache.store: "disk"`.
	#[builder(into)]
	pub path: Option<String>,
	/// If `true`, then the response is evaluated from a perspective of a shared cache (i.e. `private` is
	/// not cacheable and `s-maxage` is respected). If `false`, then the response is evaluated from a
	/// perspective of a single-user cache (i.e. `private` is cacheable and `s-maxage` is ignored).
	/// `shared: true` is required for proxies and multi-user caches.
	///
	/// Default: true.
	pub shared: Option<bool>,
}

/// Where the HTTP cache keeps its entries.
#[cfg(feature = "cache")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheStore {
	/// On disk, at `cache.path`.
	Disk,

	/// In memory, bounded by `cache.capacity`.
	Memory,
}

/// A fixed set of addresses for one domain.
#[derive(bon::Builder, Clone, Debug, Default)]
#[non_exhaustive]
pub struct DnsOverride {
	/// The domain to override.
	#[builder(into)]
	pub domain: String,
	/// The addresses to resolve it to. Empty blocks the domain.
	#[builder(with = |items: impl IntoIterator<Item = impl Into<String>>| items.into_iter().map(Into::into).collect())]
	pub addresses: Vec<String>,
}

/// DNS settings.
#[derive(bon::Builder, Clone, Debug, Default)]
#[non_exhaustive]
pub struct DnsOptions {
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
	#[cfg(feature = "dns")]
	pub system: Option<bool>,
	/// Override DNS resolution for specific domains. This takes effect even with `dns.system: true`.
	///
	/// Building the agent fails if an address is malformed. You may provide a port number as part of
	/// the address, it will default to port 0 otherwise, which will select the conventional port for the
	/// protocol in use (e.g. 80 for plaintext HTTP). If the URL passed to `fetch()` has an explicit port
	/// number, that one will be used instead. Resolving a domain to an empty `addresses` array effectively
	/// blocks that domain from this agent.
	///
	/// Default: no overrides.
	#[builder(with = |items: impl IntoIterator<Item = DnsOverride>| items.into_iter().collect())]
	pub overrides: Option<Vec<DnsOverride>>,
	/// An ordered list of resolver URLs, each URL's scheme selecting the transport Faith speaks to
	/// that resolver: `udp://` and `tcp://` for conventional DNS on port 53, `tls://` for DNS over
	/// TLS on port 853, `https://` for DNS over HTTPS on port 443, `quic://` for DNS over QUIC on
	/// port 853, and `h3://` for DNS over HTTP/3 on port 443. A port in the URL overrides the
	/// conventional one, and the HTTP transports use `/dns-query` when the URL supplies no path.
	///
	/// The encrypted transports always authenticate the resolver. A URL fragment gives the
	/// certificate to expect (`tls://1.1.1.1#cloudflare-dns.com`); a hostname host authenticates
	/// against the hostname; a bare-IP host authenticates against the address itself.
	///
	/// Servers are queried in order, a later one reached only once those before it fail. Setting
	/// this replaces the system's servers, so no discovery runs. Building the agent fails if a URL is
	/// unparseable, if its scheme is not one of the above, or if this is combined with `dns.system`.
	///
	/// Default: system discovery.
	#[cfg(feature = "dns")]
	#[builder(with = |items: impl IntoIterator<Item = impl Into<String>>| items.into_iter().map(Into::into).collect())]
	pub servers: Option<Vec<String>>,
	/// Bound name resolution across the whole server list, in milliseconds. Exhausting several dead
	/// servers costs a single timeout rather than one per server.
	///
	/// Default: 5000.
	#[cfg(feature = "dns")]
	#[builder(with = |t: std::time::Duration| millis(t))]
	pub timeout: Option<u32>,
	/// Replace the system's search list, the domains appended to a name that is not fully
	/// qualified. Independent of `dns.servers`.
	///
	/// Default: the system's search list.
	#[cfg(feature = "dns")]
	#[builder(with = |items: impl IntoIterator<Item = impl Into<String>>| items.into_iter().map(Into::into).collect())]
	pub search_domains: Option<Vec<String>>,
	/// How many dots a name must contain before it is tried as given, ahead of the search list.
	/// Independent of `dns.servers`.
	///
	/// Default: the system's setting.
	#[cfg(feature = "dns")]
	pub ndots: Option<u32>,
	/// Turn hosts-file lookup on or off. When unset, follows the platform's own convention.
	///
	/// Default: platform convention.
	#[cfg(feature = "dns")]
	pub hosts_file: Option<bool>,
	/// Further domains to exempt from the configured or encrypted resolver, for the internal
	/// suffixes a network uses. Added to the always-exempt `localhost`, `.local`, and the network's
	/// own DNS suffix; a domain is exempt when it matches an entry exactly or is a subdomain of one.
	///
	/// Default: no extra exemptions.
	#[cfg(feature = "dns")]
	#[builder(with = |items: impl IntoIterator<Item = impl Into<String>>| items.into_iter().map(Into::into).collect())]
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
	#[cfg(feature = "dns")]
	pub serve_stale: Option<bool>,
	/// How far past expiry an answer may still be served, in milliseconds. An entry older than this
	/// is discarded rather than served: an answer stale enough stops being evidence about where the
	/// host is, and a refresh still failing after that long is the case where the address most likely
	/// did change.
	///
	/// Default: 3600000 (one hour).
	#[cfg(feature = "dns")]
	#[builder(with = |t: std::time::Duration| millis(t))]
	pub max_stale: Option<u32>,
}

/// One header, as an agent default.
///
/// An invalid name or value is silently omitted. Mark a sensitive header (`Authorization`, say) so
/// it stays out of logs and HPACK's index.
#[derive(bon::Builder, Clone, Debug, Default)]
#[non_exhaustive]
pub struct Header {
	/// The header name.
	#[builder(into)]
	pub name: String,
	/// The header value.
	#[builder(into)]
	pub value: String,
	/// Whether to mark the header sensitive, keeping it out of logs and HPACK's index.
	pub sensitive: Option<bool>,
}

/// The QUIC congestion-control algorithm.
#[cfg(feature = "http3")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Http3Congestion {
	/// CUBIC, as the Linux TCP stack uses. Fair, and the default.
	#[default]
	Cubic,

	/// BBRv1, which maximises bandwidth use and ignores packet loss. See `http3.congestion`.
	Bbr1,
}

/// An assertion that HTTP/3 is available at a host and port.
///
/// Seeds the Alt-Svc cache, so the first request to that host attempts HTTP/3 without waiting for
/// an advertisement or a probe.
#[cfg(feature = "http3")]
#[derive(bon::Builder, Clone, Debug, Default)]
#[non_exhaustive]
pub struct Http3Hint {
	/// The hostname (e.g., "example.com").
	#[builder(into)]
	pub host: String,
	/// The port number (e.g., 443).
	pub port: u16,
}

/// HTTP/3 settings.
#[cfg(feature = "http3")]
#[derive(bon::Builder, Clone, Debug, Default)]
#[non_exhaustive]
pub struct Http3Options {
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
	#[builder(with = |t: std::time::Duration| u8::try_from(t.as_secs()).unwrap_or(u8::MAX))]
	pub max_idle_timeout: Option<u8>,
	/// Whether HTTP/3 upgrade via Alt-Svc is enabled. When enabled, the agent will track Alt-Svc
	/// headers from responses and automatically upgrade subsequent requests to HTTP/3 when available.
	///
	/// Default: true.
	pub upgrade_enabled: Option<bool>,
	/// Whether advertised HTTP/3 endpoints are verified with a background probe
	/// before any foreground request is routed to them.
	///
	/// An advertisement says the server listens on UDP, not that there is UDP connectivity
	/// between you and it. Without probing, the next request attempts HTTP/3 inline, and on a
	/// silently broken path it stalls until the QUIC idle timeout or `upgradeAttemptTimeout`
	/// before falling back to TCP — once per failure cooldown, for as long as the path stays
	/// broken.
	///
	/// With probing, requests keep to TCP until a background `HEAD /` over HTTP/3 confirms the
	/// path. The probe shares the connection pool, so the first upgraded request rides its warm
	/// connection, and a broken path costs one background request per cooldown and no foreground
	/// latency.
	///
	/// The probe is a synthetic request the server will see in its logs. Set `false` to restore
	/// the inline upgrade where that is unacceptable (per-request billing, easily-alarmed WAFs).
	///
	/// `hints` are exempt either way, so an origin with no TCP listener stays reachable.
	///
	/// Default: true.
	pub upgrade_probe: Option<bool>,
	/// Ceiling on how long a background HTTP/3 probe may take before the origin
	/// is treated as failed, in **milliseconds**.
	///
	/// Bounds background work only, so it can afford to be generous: a healthy handshake plus
	/// HEAD completes in one or two round trips. Set to 0 to leave probes bounded only by the
	/// QUIC idle timeout.
	///
	/// Default: 5000 (5 seconds).
	#[builder(with = |t: std::time::Duration| millis(t))]
	pub upgrade_probe_timeout: Option<u32>,
	/// Demote an origin off HTTP/3 when its QUIC path is provenly slower than
	/// its TCP path by this factor. Set to 0 to disable path-time demotion.
	///
	/// Compares a per-origin moving average of time-to-response-headers per protocol family.
	/// HTTP/3 is preferred at parity and when moderately slower, so keep this well above 1. Only
	/// a sustained gap acts: at least 8 samples each side, and an absolute 10ms, so LAN-fast
	/// origins don't flap on noise.
	///
	/// A demoted origin is not treated as broken; it re-enters through a background probe after
	/// `upgradeSlowTtl`.
	///
	/// Default: 2.5.
	pub upgrade_slow_factor: Option<f64>,
	/// How long (in seconds) a path-time demotion holds before the origin is
	/// re-evaluated. See `upgradeSlowFactor`.
	///
	/// Default: 600 (10 minutes).
	#[builder(with = |t: std::time::Duration| secs(t))]
	pub upgrade_slow_ttl: Option<u32>,
	/// How long (in seconds) to cache an Alt-Svc advertisement before the first HTTP/3 attempt.
	/// This is overridden by the `ma` (max-age) parameter in the Alt-Svc header if present.
	///
	/// Default: 86400 (24 hours).
	#[builder(with = |t: std::time::Duration| secs(t))]
	pub upgrade_advertised_ttl: Option<u32>,
	/// How long (in seconds) to cache a confirmed working HTTP/3 connection.
	///
	/// Default: 86400 (24 hours).
	#[builder(with = |t: std::time::Duration| secs(t))]
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
	#[builder(with = |t: std::time::Duration| secs(t))]
	pub upgrade_failed_ttl: Option<u32>,
	/// Ceiling (in seconds) on the cooldown that consecutive HTTP/3 failures double out of
	/// `upgradeFailedTtl`.
	///
	/// On the defaults an origin that keeps failing is blocked for 5 minutes, then 10, 20,
	/// 40, and an hour thereafter. Set this at or below `upgradeFailedTtl` for a flat
	/// cooldown that never backs off.
	///
	/// Default: 3600 (1 hour).
	#[builder(with = |t: std::time::Duration| secs(t))]
	pub upgrade_failed_max_ttl: Option<u32>,
	/// How many consecutive cancelled HTTP/3 attempts, within a 60-second window,
	/// demote an origin back to TCP.
	///
	/// A failed attempt is how a broken path is normally learned, and a cancelled request never
	/// produces one. Cancellations are weak evidence, so only a sustained run demotes the origin,
	/// and any successful HTTP/3 response resets the count.
	///
	/// Strikes must land within about a minute of each other to count towards a run, so a retry
	/// loop with a longer backoff never accumulates one; set this to 1 there.
	///
	/// One fault neither this nor `upgradeAttemptTimeout` catches: a path that
	/// carries small datagrams but drops full-size ones (an MTU blackhole, say).
	/// Response headers still arrive, so the attempt resolves and every mechanism
	/// here counts it a success — the transfer then stalls partway through the
	/// body, where nothing is watching. `maxIdleTimeout` or the request's own
	/// timeout ends such a request, and the origin stays on HTTP/3.
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
	#[builder(with = |t: std::time::Duration| millis(t))]
	pub upgrade_attempt_timeout: Option<u32>,
	/// Connect to the port a server advertises HTTP/3 on, even when it differs from
	/// the origin's own port. **This is not standards-compliant**; it is off by
	/// default.
	///
	/// An `Alt-Svc` advertisement gives a network endpoint for the origin, so
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
	#[builder(with = |items: impl IntoIterator<Item = Http3Hint>| items.into_iter().collect())]
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

/// HTTP/2 settings.
#[derive(bon::Builder, Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct Http2Options {
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
	/// Usually the wrong move. A fresh connection opens at 64 KiB, 96 times below the static
	/// default, and doubles only when a ping sample reaches two thirds of the current estimate, so
	/// it takes many round trips to ramp up and carries *less* throughput than the static window
	/// for all but the largest transfers. It also takes over both windows, so `streamWindow` and
	/// `connectionWindow` stop applying.
	///
	/// Its advantage is memory: a large window stays open only on connections that need one.
	/// Since it caps at 16 MiB, a static window near that ceiling buys the same throughput from
	/// the first byte.
	///
	/// HTTP/3 is unaffected either way, and keeps whichever windows apply to it.
	///
	/// Default: `false`.
	pub adaptive_window: Option<bool>,
}

/// Flow-control settings, shared by HTTP/2 and HTTP/3.
#[derive(bon::Builder, Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct FlowControlOptions {
	/// Maximum bytes an origin may send on any one stream before it must wait for Faith to
	/// acknowledge them, for HTTP/2 and HTTP/3 alike.
	///
	/// Larger windows keep a high-latency link full, at the cost of buffering more per stream.
	/// The default follows browser practice, at the conservative end of it:
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

/// Connection pool settings.
#[derive(bon::Builder, Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct PoolOptions {
	/// How many seconds of inactivity before a connection is closed.
	///
	/// Default: 90 seconds.
	#[builder(with = |t: std::time::Duration| secs(t))]
	pub idle_timeout: Option<u32>,
	/// The maximum amount of idle connections per host to allow in the pool. Connections will be closed
	/// to keep the idle connections (per host) under that number.
	///
	/// Default: no limit.
	pub max_idle_per_host: Option<u32>,
}

/// Switches that depart from standard behaviour.
///
/// Each quirk trades a rule Faith otherwise upholds for a capability the rule forbids. All are off
/// by default. Turning one on means requests may fail against origins that expect the standard
/// behaviour, so they suit a caller who controls the origin.
#[derive(bon::Builder, Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct QuirksOptions {
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

/// Request timeouts.
#[derive(bon::Builder, Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct TimeoutOptions {
	/// Set a timeout for only the connect phase, in milliseconds.
	///
	/// Default: none.
	#[builder(with = |t: std::time::Duration| millis(t))]
	pub connect: Option<u32>,
	/// Set a timeout for read operations, in milliseconds.
	///
	/// The timeout applies to each read operation, and resets after a successful read. This is more
	/// appropriate for detecting stalled connections when the size isn't known beforehand.
	///
	/// Default: none.
	#[builder(with = |t: std::time::Duration| millis(t))]
	pub read: Option<u32>,
	/// Set a timeout for the entire request-response cycle, in milliseconds.
	///
	/// The timeout applies from when the request starts connecting until the response body has finished.
	/// Also considered a total deadline.
	///
	/// Default: none.
	#[builder(with = |t: std::time::Duration| millis(t))]
	pub total: Option<u32>,
}

/// TLS settings.
#[derive(bon::Builder, Clone, Debug, Default)]
#[non_exhaustive]
pub struct TlsOptions {
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
	/// private key must be in RSA, SEC1 Elliptic Curve or PKCS#8 format. This is one of the few
	/// options whose input is parsed at construction, so a malformed one fails the build.
	#[builder(into)]
	pub identity: Option<Vec<u8>>,
	/// Disables plain-text HTTP.
	///
	/// Default: false.
	pub required: Option<bool>,
	/// Additional PEM-formatted root certificates to trust, on top of the platform's
	/// trust store. Each entry may be a PEM bundle containing multiple certificates.
	///
	/// This is mainly useful for connecting to servers with self-signed or private-CA
	/// certificates, such as internal services or local test servers. This is one of the
	/// few options whose input is parsed at construction, so a malformed one fails the
	/// build.
	#[builder(with = |items: impl IntoIterator<Item = impl Into<Vec<u8>>>| items.into_iter().map(Into::into).collect())]
	pub extra_roots: Option<Vec<Vec<u8>>>,
}

/// Everything an agent can be configured with.
#[derive(bon::Builder, Clone, Debug, Default)]
#[builder(
	builder_type(
		name = AgentOptionsBuilder,
		doc {
			/// Builds an [`Agent`](crate::Agent), a setting at a time.
			///
			/// Each option group is reached through a closure, so a group left alone is absent
			/// from the call rather than spelled out as absent. Anything unset takes its default.
			/// [`build`](Self::build) validates the settings and produces the agent.
		}
	),
	finish_fn(name = into_options_inner, vis = "pub(crate)"),
	state_mod(vis = "pub")
)]
#[non_exhaustive]
pub struct AgentOptions {
	/// HTTP cache settings.
	#[cfg(feature = "cache")]
	#[builder(with = |with: impl FnOnce(CacheOptionsBuilder) -> CacheOptions| with(CacheOptions::builder()))]
	pub cache: Option<CacheOptions>,
	/// Keep a cookie jar on the agent, so cookies set by a response are sent on later requests.
	///
	/// [`CookieLimits::default()`] takes the default caps; its fields tune them, and
	/// [`Agent::cookies`](crate::agent::Agent::cookies) reaches the jar itself.
	///
	/// Default: no jar.
	#[cfg(feature = "cookies")]
	pub cookies: Option<CookieLimits>,
	/// DNS settings.
	#[builder(with = |with: impl FnOnce(DnsOptionsBuilder) -> DnsOptions| with(DnsOptions::builder()))]
	pub dns: Option<DnsOptions>,
	/// Flow-control windows shared by HTTP/2 and HTTP/3.
	///
	/// Setting these is the normal way to tune windows: one value applies to whichever protocol
	/// a request negotiates, so throughput doesn't change when an origin upgrades from one to
	/// the other. The `http2` and `http3` groups override them per protocol.
	#[builder(with = |with: impl FnOnce(FlowControlOptionsBuilder) -> FlowControlOptions| with(FlowControlOptions::builder()))]
	pub flow_control: Option<FlowControlOptions>,
	/// Sets the default headers for every request.
	///
	/// If header names or values are invalid, they are silently omitted.
	/// Sensitive headers (e.g. `Authorization`) should be marked.
	///
	/// Default: none.
	#[builder(with = |items: impl IntoIterator<Item = Header>| items.into_iter().collect())]
	pub headers: Option<Vec<Header>>,
	/// HTTP/2 settings.
	#[builder(with = |with: impl FnOnce(Http2OptionsBuilder) -> Http2Options| with(Http2Options::builder()))]
	pub http2: Option<Http2Options>,
	/// HTTP/3 settings.
	#[cfg(feature = "http3")]
	#[builder(with = |with: impl FnOnce(Http3OptionsBuilder) -> Http3Options| with(Http3Options::builder()))]
	pub http3: Option<Http3Options>,
	/// Bind outgoing sockets to this local IP address before connecting.
	///
	/// This also selects the address family of the HTTP/3 (QUIC) socket. By default that
	/// socket binds the IPv6 wildcard (`[::]`), which fails on hosts without usable IPv6 —
	/// there, HTTP/3 silently falls back to TCP. Faith detects that case automatically and
	/// binds `0.0.0.0` instead, so you normally don't need to set this; provide it only to
	/// force a specific source address.
	///
	/// Default: unset (IPv6 wildcard for QUIC where available, else `0.0.0.0`).
	pub local_address: Option<std::net::IpAddr>,
	/// Connection pool settings.
	#[builder(with = |with: impl FnOnce(PoolOptionsBuilder) -> PoolOptions| with(PoolOptions::builder()))]
	pub pool: Option<PoolOptions>,
	/// Switches that depart from standard behaviour.
	#[builder(with = |with: impl FnOnce(QuirksOptionsBuilder) -> QuirksOptions| with(QuirksOptions::builder()))]
	pub quirks: Option<QuirksOptions>,
	/// Determines the behavior in case the server replies with a redirect status.
	pub redirect: Option<RedirectPolicy>,
	/// Request timeouts.
	#[builder(with = |with: impl FnOnce(TimeoutOptionsBuilder) -> TimeoutOptions| with(TimeoutOptions::builder()))]
	pub timeout: Option<TimeoutOptions>,
	/// TLS settings.
	#[builder(with = |with: impl FnOnce(TlsOptionsBuilder) -> TlsOptions| with(TlsOptions::builder()))]
	pub tls: Option<TlsOptions>,
	/// Custom user agent string.
	///
	/// Default: `Faith/{version} reqwest/{version}`.
	#[builder(into)]
	pub user_agent: Option<String>,
}

/// Whether this host can bind the IPv6 wildcard (`[::]`).
///
/// Tested with the same operation reqwest performs when creating a QUIC endpoint with no explicit
/// local address, so it predicts whether the default bind will succeed. Memoised for the life of
/// the process.
pub fn ipv6_wildcard_bindable() -> bool {
	use std::sync::OnceLock;
	static BINDABLE: OnceLock<bool> = OnceLock::new();
	*BINDABLE.get_or_init(|| {
		UdpSocket::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)).is_ok()
	})
}

/// Reconcile one protocol's windows: its own setting wins over the common one, which wins over the
/// default.
// spec:FLOW#per-protocol-windows
pub(crate) fn resolve_windows(
	common: Option<&FlowControlOptions>,
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::client::ResolvedWindows;

	fn common(stream: Option<u32>, connection: Option<u32>) -> FlowControlOptions {
		FlowControlOptions {
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
