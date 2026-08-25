//! The `Agent` class, as JavaScript sees it.

use napi::bindgen_prelude::{PromiseRaw, within_runtime_if_available};

use napi::Env;
use napi_derive::napi;

use crate::{
	async_task::faith_promise,
	error::{FaithError, FaithErrorExt},
};

#[cfg(feature = "connection-tracking")]
use crate::conn_tracker::{ConnectionInfo, connections_for_napi};

mod convert;
mod options;

pub use options::*;

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
pub const USER_AGENT: &str = web_faith::USER_AGENT;

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
#[cfg(feature = "dns")]
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
		refuse_absent_capabilities(&options)?;
		let options = web_faith::options::AgentOptions::from(options);
		// A napi callback can run outside the runtime, and building the HTTP/3 endpoint needs to be
		// inside one, so the client is constructed within whichever runtime is to hand.
		within_runtime_if_available(|| web_faith::agent::Agent::from_options(options))
			.map(|inner| Self { inner })
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

/// Refuse an option group this build cannot honour.
///
/// A Cargo feature drops the capability and, on the Rust surface, the API that reaches it. napi's
/// object derive does not honour `#[cfg]` on a field, so an options object here keeps its full shape
/// whatever the build; asking for a capability that is not compiled in is refused rather than
/// quietly ignored, so a slim build says so instead of appearing to work.
fn refuse_absent_capabilities(options: &AgentOptions) -> Result<(), FaithError> {
	let absent = |group: &str| -> Result<(), FaithError> {
		Err(FaithError::new(
			web_faith::FaithErrorKind::Config,
			Some(format!("this build has no {group} support")),
		))
	};

	#[cfg(not(feature = "cache"))]
	if options.cache.is_some() {
		return absent("HTTP cache");
	}

	#[cfg(not(feature = "cookies"))]
	if options.cookies.is_some() {
		return absent("cookie");
	}

	// `dns.overrides` reaches reqwest rather than Faith's resolver, so it is honoured either way;
	// every other setting in the group configures the resolver this build does not have.
	#[cfg(not(feature = "dns"))]
	if options.dns.as_ref().is_some_and(|dns| {
		dns.system.is_some()
			|| dns.servers.is_some()
			|| dns.timeout.is_some()
			|| dns.search_domains.is_some()
			|| dns.ndots.is_some()
			|| dns.hosts_file.is_some()
			|| dns.exempt_domains.is_some()
			|| dns.serve_stale.is_some()
			|| dns.max_stale.is_some()
	}) {
		return absent("resolver");
	}

	let _ = (options, absent);
	Ok(())
}

/// Per-connection reporting, which needs the tracker that gathers it.
#[cfg(feature = "connection-tracking")]
#[napi]
impl Agent {
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
}

/// The resolver's own observability, which needs a resolver of Faith's own to report on.
#[cfg(feature = "dns")]
#[napi]
impl Agent {
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
}

/// The cookie jar's verbs, which exist when the build keeps a jar.
#[cfg(feature = "cookies")]
use std::str::FromStr as _;

#[cfg(feature = "cookies")]
use reqwest::Url;

#[cfg(feature = "cookies")]
#[napi]
impl Agent {
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

		let Some(jar) = self.inner.cookies() else {
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
		let url = Url::from_str(&url).ok()?;
		self.inner
			.cookies()?
			.request_cookie_header(&url)
			.and_then(|value| value.to_str().ok().map(ToOwned::to_owned))
	}
}
