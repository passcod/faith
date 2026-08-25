//! Building an agent a setting at a time.
//!
//! [`Agent::builder`] returns an [`AgentBuilder`], whose methods mirror the option groups. A group
//! is reached through a closure, so one left alone is absent from the call rather than spelled out
//! as absent:
//!
//! ```no_run
//! # use std::time::Duration;
//! # use web_faith::agent::Agent;
//! let agent = Agent::builder()
//!     .user_agent("YourApp/1.2.3")
//!     .timeout(|timeout| timeout.connect(Duration::from_secs(2)))
//!     .pool(|pool| pool.max_idle_per_host(8))
//!     .build()?;
//! # Ok::<(), web_faith::FaithError>(())
//! ```
//!
//! Durations are `Duration` here whatever unit the setting is carried in, and a setting left unset
//! takes its default.
//!
//! [`Agent::builder`]: crate::agent::Agent::builder

// spec:AGENT

use std::{net::IpAddr, time::Duration};

#[cfg(feature = "cache")]
use http_cache_reqwest::CacheMode;

#[cfg(feature = "cache")]
use crate::options::{CacheOptions, CacheStore};

#[cfg(feature = "http3")]
use crate::options::{Http3Congestion, Http3Hint, Http3Options};

#[cfg(feature = "cookies")]
use web_faith_cookies::CookieLimits;

use crate::{
	agent::Agent,
	client::RedirectPolicy,
	error::FaithError,
	options::{
		AgentOptions, DnsOptions, DnsOverride, FlowControlOptions, Header, Http2Options,
		PoolOptions, QuirksOptions, TimeoutOptions, TlsOptions,
	},
};

/// Milliseconds, saturating rather than wrapping on a duration no setting could mean.
fn millis(duration: Duration) -> u32 {
	duration.as_millis().try_into().unwrap_or(u32::MAX)
}

/// Whole seconds, rounded down, saturating as above.
fn secs(duration: Duration) -> u32 {
	duration.as_secs().try_into().unwrap_or(u32::MAX)
}

/// Builds an [`Agent`]. See [the module documentation](self).
#[derive(Debug, Default)]
#[must_use = "an agent builder does nothing until built"]
pub struct AgentBuilder {
	options: AgentOptions,
}

impl AgentBuilder {
	/// Validate what has been set and build the agent.
	pub fn build(self) -> Result<Agent, FaithError> {
		Agent::from_options(self.options)
	}

	/// The options as they stand, for a caller that would rather fill them in directly.
	pub fn into_options(self) -> AgentOptions {
		self.options
	}

	/// The `User-Agent` requests carry. Prepend to [`crate::USER_AGENT`] rather than replacing it,
	/// so a server still sees which client is calling.
	pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
		self.options.user_agent = Some(user_agent.into());
		self
	}

	/// The local address to bind connections to.
	pub fn local_address(mut self, address: IpAddr) -> Self {
		self.options.local_address = Some(address.to_string());
		self
	}

	/// Add a default header, sent on every request that does not set one of that name itself.
	pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
		self.options.headers.get_or_insert_default().push(Header {
			name: name.into(),
			value: value.into(),
			sensitive: None,
		});
		self
	}

	/// Add a default header, marking it sensitive so it is kept out of logs and HPACK indexes.
	pub fn sensitive_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
		self.options.headers.get_or_insert_default().push(Header {
			name: name.into(),
			value: value.into(),
			sensitive: Some(true),
		});
		self
	}

	/// What to do with a redirect response.
	pub fn redirect(mut self, policy: RedirectPolicy) -> Self {
		self.options.redirect = Some(policy);
		self
	}

	/// Keep a cookie jar, enforcing these limits. Without this call the agent stores no cookies.
	#[cfg(feature = "cookies")]
	pub fn cookies(mut self, limits: CookieLimits) -> Self {
		self.options.cookies = Some(limits);
		self
	}

	/// Resolver settings.
	pub fn dns(mut self, with: impl FnOnce(DnsBuilder) -> DnsBuilder) -> Self {
		let group = self.options.dns.take().unwrap_or_default();
		self.options.dns = Some(with(DnsBuilder { group }).group);
		self
	}

	/// TLS settings.
	pub fn tls(mut self, with: impl FnOnce(TlsBuilder) -> TlsBuilder) -> Self {
		let group = self.options.tls.take().unwrap_or_default();
		self.options.tls = Some(with(TlsBuilder { group }).group);
		self
	}

	/// Connection pool settings.
	pub fn pool(mut self, with: impl FnOnce(PoolBuilder) -> PoolBuilder) -> Self {
		let group = self.options.pool.take().unwrap_or_default();
		self.options.pool = Some(with(PoolBuilder { group }).group);
		self
	}

	/// Request timeouts.
	pub fn timeout(mut self, with: impl FnOnce(TimeoutBuilder) -> TimeoutBuilder) -> Self {
		let group = self.options.timeout.take().unwrap_or_default();
		self.options.timeout = Some(with(TimeoutBuilder { group }).group);
		self
	}

	/// HTTP cache settings. Without this call the agent does not cache.
	#[cfg(feature = "cache")]
	pub fn cache(mut self, with: impl FnOnce(CacheBuilder) -> CacheBuilder) -> Self {
		let group = self.options.cache.take().unwrap_or_default();
		self.options.cache = Some(with(CacheBuilder { group }).group);
		self
	}

	/// Flow-control windows applied to both protocols, unless one overrides them.
	pub fn flow_control(
		mut self,
		with: impl FnOnce(FlowControlBuilder) -> FlowControlBuilder,
	) -> Self {
		let group = self.options.flow_control.take().unwrap_or_default();
		self.options.flow_control = Some(with(FlowControlBuilder { group }).group);
		self
	}

	/// HTTP/2 settings.
	pub fn http2(mut self, with: impl FnOnce(Http2Builder) -> Http2Builder) -> Self {
		let group = self.options.http2.take().unwrap_or_default();
		self.options.http2 = Some(with(Http2Builder { group }).group);
		self
	}

	/// HTTP/3 settings.
	#[cfg(feature = "http3")]
	pub fn http3(mut self, with: impl FnOnce(Http3Builder) -> Http3Builder) -> Self {
		let group = self.options.http3.take().unwrap_or_default();
		self.options.http3 = Some(with(Http3Builder { group }).group);
		self
	}

	/// Departures from standard behaviour, each opted into deliberately.
	pub fn quirks(mut self, with: impl FnOnce(QuirksBuilder) -> QuirksBuilder) -> Self {
		let group = self.options.quirks.take().unwrap_or_default();
		self.options.quirks = Some(with(QuirksBuilder { group }).group);
		self
	}
}

/// Resolver settings. Reached through [`AgentBuilder::dns`].
#[derive(Debug, Default)]
#[must_use]
pub struct DnsBuilder {
	group: DnsOptions,
}

impl DnsBuilder {
	/// Resolve through the operating system rather than Faith's own resolver.
	#[cfg(feature = "dns")]
	pub fn system(mut self, system: bool) -> Self {
		self.group.system = Some(system);
		self
	}

	/// Resolve `domain` to these addresses without asking a server.
	pub fn r#override(
		mut self,
		domain: impl Into<String>,
		addresses: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		self.group
			.overrides
			.get_or_insert_default()
			.push(DnsOverride {
				domain: domain.into(),
				addresses: addresses.into_iter().map(Into::into).collect(),
			});
		self
	}

	/// The resolvers to query, in order. Each is a URL whose scheme picks the transport.
	#[cfg(feature = "dns")]
	pub fn servers(mut self, servers: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.group.servers = Some(servers.into_iter().map(Into::into).collect());
		self
	}

	/// Bound resolution across the whole server list.
	#[cfg(feature = "dns")]
	pub fn timeout(mut self, timeout: Duration) -> Self {
		self.group.timeout = Some(millis(timeout));
		self
	}

	/// Suffixes to try for an unqualified name.
	#[cfg(feature = "dns")]
	pub fn search_domains(mut self, domains: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.group.search_domains = Some(domains.into_iter().map(Into::into).collect());
		self
	}

	/// How many dots a name must contain before it is tried as given, ahead of the search list.
	#[cfg(feature = "dns")]
	pub fn ndots(mut self, ndots: u32) -> Self {
		self.group.ndots = Some(ndots);
		self
	}

	/// Consult the system hosts file.
	#[cfg(feature = "dns")]
	pub fn hosts_file(mut self, hosts_file: bool) -> Self {
		self.group.hosts_file = Some(hosts_file);
		self
	}

	/// Names to send to the system resolver whatever the rest of the configuration says.
	#[cfg(feature = "dns")]
	pub fn exempt_domains(mut self, domains: impl IntoIterator<Item = impl Into<String>>) -> Self {
		self.group.exempt_domains = Some(domains.into_iter().map(Into::into).collect());
		self
	}

	/// Serve an expired answer while a fresh lookup runs.
	#[cfg(feature = "dns")]
	pub fn serve_stale(mut self, serve_stale: bool) -> Self {
		self.group.serve_stale = Some(serve_stale);
		self
	}

	/// How far past expiry an answer may still be served.
	#[cfg(feature = "dns")]
	pub fn max_stale(mut self, max_stale: Duration) -> Self {
		self.group.max_stale = Some(millis(max_stale));
		self
	}
}

/// TLS settings. Reached through [`AgentBuilder::tls`].
#[derive(Debug, Default)]
#[must_use]
pub struct TlsBuilder {
	group: TlsOptions,
}

impl TlsBuilder {
	/// Send early data on resumed connections, which trades a round trip for replayability.
	pub fn early_data(mut self, early_data: bool) -> Self {
		self.group.early_data = Some(early_data);
		self
	}

	/// A PEM certificate and private key to present as a client certificate.
	pub fn identity(mut self, pem: impl Into<Vec<u8>>) -> Self {
		self.group.identity = Some(pem.into());
		self
	}

	/// Trust this PEM root on top of the platform's trust store.
	pub fn extra_root(mut self, pem: impl Into<Vec<u8>>) -> Self {
		self.group
			.extra_roots
			.get_or_insert_default()
			.push(pem.into());
		self
	}

	/// Refuse a connection that cannot be made over TLS.
	pub fn required(mut self, required: bool) -> Self {
		self.group.required = Some(required);
		self
	}
}

/// Connection pool settings. Reached through [`AgentBuilder::pool`].
#[derive(Debug, Default)]
#[must_use]
pub struct PoolBuilder {
	group: PoolOptions,
}

impl PoolBuilder {
	/// How long a connection may sit idle before it is closed.
	pub fn idle_timeout(mut self, idle_timeout: Duration) -> Self {
		self.group.idle_timeout = Some(secs(idle_timeout));
		self
	}

	/// Most idle connections to keep per host.
	pub fn max_idle_per_host(mut self, max: u32) -> Self {
		self.group.max_idle_per_host = Some(max);
		self
	}
}

/// Request timeouts. Reached through [`AgentBuilder::timeout`].
#[derive(Debug, Default)]
#[must_use]
pub struct TimeoutBuilder {
	group: TimeoutOptions,
}

impl TimeoutBuilder {
	/// Bound the connect phase alone.
	pub fn connect(mut self, timeout: Duration) -> Self {
		self.group.connect = Some(millis(timeout));
		self
	}

	/// Bound each read.
	pub fn read(mut self, timeout: Duration) -> Self {
		self.group.read = Some(millis(timeout));
		self
	}

	/// Bound the whole request and response.
	pub fn total(mut self, timeout: Duration) -> Self {
		self.group.total = Some(millis(timeout));
		self
	}
}

/// HTTP cache settings. Reached through [`AgentBuilder::cache`].
#[cfg(feature = "cache")]
#[derive(Debug, Default)]
#[must_use]
pub struct CacheBuilder {
	group: CacheOptions,
}

#[cfg(feature = "cache")]
impl CacheBuilder {
	/// Where cached responses are kept.
	pub fn store(mut self, store: CacheStore) -> Self {
		self.group.store = Some(store);
		self
	}

	/// For an in-memory store, how many entries to keep.
	pub fn capacity(mut self, capacity: u32) -> Self {
		self.group.capacity = Some(capacity);
		self
	}

	/// The default cache mode for requests that name none.
	pub fn mode(mut self, mode: CacheMode) -> Self {
		self.group.mode = Some(mode);
		self
	}

	/// For a disk store, the directory to keep it in.
	pub fn path(mut self, path: impl Into<String>) -> Self {
		self.group.path = Some(path.into());
		self
	}

	/// Behave as a shared cache rather than a private one.
	pub fn shared(mut self, shared: bool) -> Self {
		self.group.shared = Some(shared);
		self
	}
}

/// Flow-control windows. Reached through [`AgentBuilder::flow_control`].
#[derive(Debug, Default)]
#[must_use]
pub struct FlowControlBuilder {
	group: FlowControlOptions,
}

impl FlowControlBuilder {
	/// Bytes an origin may send on any one stream before waiting.
	pub fn stream_window(mut self, bytes: u32) -> Self {
		self.group.stream_window = Some(bytes);
		self
	}

	/// Bytes an origin may send across all streams of one connection before waiting.
	pub fn connection_window(mut self, bytes: u32) -> Self {
		self.group.connection_window = Some(bytes);
		self
	}
}

/// HTTP/2 settings. Reached through [`AgentBuilder::http2`].
#[derive(Debug, Default)]
#[must_use]
pub struct Http2Builder {
	group: Http2Options,
}

impl Http2Builder {
	/// Bytes an origin may send on any one HTTP/2 stream before waiting.
	pub fn stream_window(mut self, bytes: u32) -> Self {
		self.group.stream_window = Some(bytes);
		self
	}

	/// Bytes an origin may send across all streams of one HTTP/2 connection before waiting.
	pub fn connection_window(mut self, bytes: u32) -> Self {
		self.group.connection_window = Some(bytes);
		self
	}

	/// Let the windows size themselves from what the connection is actually doing.
	pub fn adaptive_window(mut self, adaptive: bool) -> Self {
		self.group.adaptive_window = Some(adaptive);
		self
	}
}

/// HTTP/3 settings. Reached through [`AgentBuilder::http3`].
#[cfg(feature = "http3")]
#[derive(Debug, Default)]
#[must_use]
pub struct Http3Builder {
	group: Http3Options,
}

#[cfg(feature = "http3")]
impl Http3Builder {
	/// The congestion controller QUIC runs.
	pub fn congestion(mut self, congestion: Http3Congestion) -> Self {
		self.group.congestion = Some(congestion);
		self
	}

	/// Inactivity to accept before timing out a connection. Rounded down to whole seconds, and
	/// capped at what the setting can carry.
	pub fn max_idle_timeout(mut self, timeout: Duration) -> Self {
		self.group.max_idle_timeout = Some(timeout.as_secs().try_into().unwrap_or(u8::MAX));
		self
	}

	/// Whether an origin advertising HTTP/3 is upgraded to it at all.
	pub fn upgrade_enabled(mut self, enabled: bool) -> Self {
		self.group.upgrade_enabled = Some(enabled);
		self
	}

	/// Verify an advertisement with a background probe rather than on the next request.
	pub fn upgrade_probe(mut self, probe: bool) -> Self {
		self.group.upgrade_probe = Some(probe);
		self
	}

	/// Ceiling on how long a background probe may take.
	pub fn upgrade_probe_timeout(mut self, timeout: Duration) -> Self {
		self.group.upgrade_probe_timeout = Some(millis(timeout));
		self
	}

	/// Demote an origin off HTTP/3 when its QUIC path is slower than its TCP one by this factor.
	pub fn upgrade_slow_factor(mut self, factor: f64) -> Self {
		self.group.upgrade_slow_factor = Some(factor);
		self
	}

	/// How long a path-time demotion holds before the origin is re-evaluated.
	pub fn upgrade_slow_ttl(mut self, ttl: Duration) -> Self {
		self.group.upgrade_slow_ttl = Some(secs(ttl));
		self
	}

	/// How long to cache an advertisement before the first HTTP/3 attempt.
	pub fn upgrade_advertised_ttl(mut self, ttl: Duration) -> Self {
		self.group.upgrade_advertised_ttl = Some(secs(ttl));
		self
	}

	/// How long to cache a confirmed working HTTP/3 connection.
	pub fn upgrade_confirmed_ttl(mut self, ttl: Duration) -> Self {
		self.group.upgrade_confirmed_ttl = Some(secs(ttl));
		self
	}

	/// How long a first failed attempt blocks an origin.
	pub fn upgrade_failed_ttl(mut self, ttl: Duration) -> Self {
		self.group.upgrade_failed_ttl = Some(secs(ttl));
		self
	}

	/// Ceiling on the cooldown consecutive failures double out to.
	pub fn upgrade_failed_max_ttl(mut self, ttl: Duration) -> Self {
		self.group.upgrade_failed_max_ttl = Some(secs(ttl));
		self
	}

	/// How many consecutive cancelled attempts demote an origin.
	pub fn upgrade_cancel_strikes(mut self, strikes: u32) -> Self {
		self.group.upgrade_cancel_strikes = Some(strikes);
		self
	}

	/// Ceiling on how long an attempt may take to resolve before it is given up on.
	pub fn upgrade_attempt_timeout(mut self, timeout: Duration) -> Self {
		self.group.upgrade_attempt_timeout = Some(millis(timeout));
		self
	}

	/// Connect to a port an origin advertised, rather than only to its own.
	pub fn upgrade_follow_advertised_port(mut self, follow: bool) -> Self {
		self.group.upgrade_follow_advertised_port = Some(follow);
		self
	}

	/// How many origins to track in the Alt-Svc cache.
	pub fn upgrade_cache_capacity(mut self, capacity: u32) -> Self {
		self.group.upgrade_cache_capacity = Some(capacity);
		self
	}

	/// Treat HTTP/3 as available at this host and port without waiting for an advertisement.
	pub fn hint(mut self, host: impl Into<String>, port: u16) -> Self {
		self.group.hints.get_or_insert_default().push(Http3Hint {
			host: host.into(),
			port,
		});
		self
	}

	/// Bytes an origin may send on any one HTTP/3 stream before waiting.
	pub fn stream_window(mut self, bytes: u32) -> Self {
		self.group.stream_window = Some(bytes);
		self
	}

	/// Bytes an origin may send across all streams of one HTTP/3 connection before waiting.
	pub fn connection_window(mut self, bytes: u32) -> Self {
		self.group.connection_window = Some(bytes);
		self
	}

	/// Bytes to transmit without acknowledgement, bounding upload memory.
	pub fn send_window(mut self, bytes: u32) -> Self {
		self.group.send_window = Some(bytes);
		self
	}
}

/// Departures from standard behaviour. Reached through [`AgentBuilder::quirks`].
#[derive(Debug, Default)]
#[must_use]
pub struct QuirksBuilder {
	group: QuirksOptions,
}

impl QuirksBuilder {
	/// Send a streaming request body over HTTP/1.x, which the fetch standard otherwise reserves to
	/// HTTP/2 and HTTP/3.
	pub fn h1_request_streaming(mut self, allow: bool) -> Self {
		self.group.h1_request_streaming = Some(allow);
		self
	}
}
