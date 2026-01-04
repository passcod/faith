use std::{
	fmt::Debug,
	net::{IpAddr, SocketAddr, SocketAddrV4, SocketAddrV6},
	str::FromStr as _,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use http_cache_reqwest::{
	CACacheManager, Cache, CacheOptions, HttpCache, HttpCacheOptions, MokaCacheBuilder, MokaManager,
};
use napi::{Either, Env, bindgen_prelude::Buffer};
use napi_derive::napi;
use reqwest::{
	Client, Identity, Url,
	cookie::{CookieStore, Jar},
	header::{HeaderMap, HeaderName, HeaderValue},
	redirect::Policy,
};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};

use crate::{
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
}

impl Debug for AgentTlsOptions {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AgentTlsOptions")
			.field("early_data", &self.early_data)
			.field("identity", &"[sensitive]")
			.field("required", &self.required)
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
	pub(crate) client: ClientWithMiddleware,
	pub(crate) cookie_jar: Option<Arc<Jar>>,
	pub(crate) stats: Arc<InnerAgentStats>,
}

#[napi]
impl Agent {
	pub fn new() -> Result<Self, FaithError> {
		Self::with_options(AgentOptions::default())
	}

	pub fn with_options(options: AgentOptions) -> Result<Self, FaithError> {
		let mut client = Client::builder()
			.tls_info(true)
			.tls_sslkeylogfile(true)
			.user_agent(options.user_agent.as_deref().unwrap_or(USER_AGENT));

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
		if let Some(http3) = options.http3 {
			if let Some(Http3Congestion::Bbr1) = http3.congestion {
				client = client.http3_congestion_bbr();
			}

			if let Some(seconds) = http3.max_idle_timeout {
				use std::time::Duration;
				client = client
					.http3_max_idle_timeout(Duration::from_secs(seconds.min(120).max(1).into()));
			}
		}

		if let Some(pool) = options.pool {
			if let Some(seconds) = pool.idle_timeout {
				client = client.pool_idle_timeout(Some(Duration::from_secs(seconds.max(0).into())));
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
		}

		let mut client = ClientBuilder::new(client.build()?);

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

		Ok(Self {
			client: client.build(),
			cookie_jar,
			stats: Default::default(),
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
}
