//! Faith's own DNS resolver.
//!
//! `reqwest`'s built-in hickory resolver and its in-memory cache are `pub(crate)`, so the only
//! way to warm that cache is to make a request through it — which is `preconnect`'s job, not
//! `prefetchDns`'s (that verb must not touch the origin). To let `prefetchDns` populate the cache
//! a later request reads, Faith owns the resolver instead: this type is installed on the `reqwest`
//! client with `ClientBuilder::dns_resolver`, so reqwest routes every lookup through it, and
//! `prefetch` calls the same resolver directly. Both share one `TokioResolver`, so a name warmed
//! by `prefetchDns` is already cached when a request looks it up.
//!
//! Beyond warming, this type is where the DNS transports and server order live. `dns.servers`
//! lists resolver URLs whose scheme picks the transport (`udp`/`tcp`, or the encrypted `tls`,
//! `https`, `quic`, `h3`); the list is queried in order, held fixed with `UserProvidedOrder`. With
//! no list, the resolver configures itself from the system and lets hickory's RFC 9539
//! opportunistic encryption upgrade those servers where it can. Either way, [exempt names] go to
//! the system resolver instead, so local names keep resolving.
//!
//! [exempt names]: ResolverSettings::exempt_domains
//!
//! spec:WARM spec:DNS

use std::{
	collections::HashSet,
	net::{IpAddr, SocketAddr},
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use hickory_resolver::{
	TokioResolver,
	config::{
		ConnectionConfig, GOOGLE, LookupIpStrategy, NameServerConfig, OpportunisticEncryption,
		ProtocolConfig, ResolveHosts, ResolverConfig, ServerOrderingStrategy,
	},
	net::{DnsError, NetError, runtime::TokioRuntimeProvider},
	proto::rr::Name,
	system_conf::read_system_conf,
};
use reqwest::dns::{Addrs, Name as ReqName, Resolve, Resolving};
use tokio::sync::OnceCell;
use url::{Host, Url};

/// The default DoH/DoQ query path, used when a `https://`/`h3://` server URL supplies none.
const DEFAULT_DNS_QUERY_PATH: &str = "/dns-query";

/// Parse a `dns.searchDomains` or `dns.exemptDomains` list into domain names, or return a message
/// for the first entry that is not a valid domain name.
pub fn parse_domains(list: Option<Vec<String>>) -> Result<Option<Vec<Name>>, String> {
	list.map(|items| {
		items
			.iter()
			.map(|item| Name::from_utf8(item).map_err(|err| format!("{item:?}: {err}")))
			.collect()
	})
	.transpose()
}

/// The transport Faith speaks to a resolver, chosen by a server URL's scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
	Udp,
	Tcp,
	Tls,
	Https,
	Quic,
	H3,
}

impl Transport {
	fn from_scheme(scheme: &str) -> Option<Self> {
		Some(match scheme {
			"udp" => Self::Udp,
			"tcp" => Self::Tcp,
			"tls" => Self::Tls,
			"https" => Self::Https,
			"quic" => Self::Quic,
			"h3" => Self::H3,
			_ => return None,
		})
	}

	/// The conventional port for the transport, used when the URL names none.
	fn default_port(self) -> u16 {
		match self {
			Self::Udp | Self::Tcp => 53,
			Self::Tls | Self::Quic => 853,
			Self::Https | Self::H3 => 443,
		}
	}

	/// The lowercase label reported by `resolvers()`.
	fn label(self) -> &'static str {
		match self {
			Self::Udp => "udp",
			Self::Tcp => "tcp",
			Self::Tls => "tls",
			Self::Https => "https",
			Self::Quic => "quic",
			Self::H3 => "h3",
		}
	}
}

/// A resolver Faith reaches by IP or by a hostname it bootstraps.
#[derive(Clone, Debug)]
enum ServerHost {
	Ip(IpAddr),
	Name(String),
}

/// One entry of `dns.servers`, parsed at agent construction. The IP is not known yet for a
/// hostname host: that is resolved when the resolver is first used (see [`ResolverSettings`]).
#[derive(Clone, Debug)]
pub struct ServerSpec {
	host: ServerHost,
	transport: Transport,
	port: u16,
	/// DoH/DoQ query path, `None` for the non-HTTP transports.
	path: Option<Arc<str>>,
	/// The name to authenticate the certificate against, from a URL fragment. When absent, a
	/// hostname host authenticates against itself and an IP host against the address.
	cert_name: Option<String>,
}

impl ServerSpec {
	/// Parse one `dns.servers` URL, or return a message for an unparseable URL or unknown scheme.
	pub fn parse(input: &str) -> Result<Self, String> {
		let url = Url::parse(input).map_err(|err| format!("{input:?}: {err}"))?;
		let transport = Transport::from_scheme(url.scheme())
			.ok_or_else(|| format!("{input:?}: unknown DNS transport scheme {:?}", url.scheme()))?;

		let host = match url.host() {
			Some(Host::Ipv4(ip)) => ServerHost::Ip(IpAddr::V4(ip)),
			Some(Host::Ipv6(ip)) => ServerHost::Ip(IpAddr::V6(ip)),
			Some(Host::Domain(name)) => ServerHost::Name(name.to_owned()),
			None => return Err(format!("{input:?}: no host to resolve")),
		};

		let port = url.port().unwrap_or_else(|| transport.default_port());
		let path = match transport {
			Transport::Https | Transport::H3 => {
				let path = url.path();
				(!path.is_empty() && path != "/").then(|| Arc::from(path))
			}
			_ => None,
		};
		let cert_name = url.fragment().map(str::to_owned);

		Ok(Self {
			host,
			transport,
			port,
			path,
			cert_name,
		})
	}

	/// The IP host, or `None` for a hostname host that still needs bootstrapping.
	fn ip(&self) -> Option<IpAddr> {
		match self.host {
			ServerHost::Ip(ip) => Some(ip),
			ServerHost::Name(_) => None,
		}
	}

	/// The certificate name to authenticate against once the host resolves to `ip`: an explicit
	/// fragment, else the hostname, else the address itself (spec:DNS#transports).
	fn server_name(&self, ip: IpAddr) -> Arc<str> {
		if let Some(name) = &self.cert_name {
			Arc::from(name.as_str())
		} else {
			match &self.host {
				ServerHost::Name(name) => Arc::from(name.as_str()),
				ServerHost::Ip(_) => Arc::from(ip.to_string()),
			}
		}
	}

	/// Build the hickory name server for this spec, reached at `ip`.
	fn to_name_server(&self, ip: IpAddr) -> NameServerConfig {
		let protocol = match self.transport {
			Transport::Udp => ProtocolConfig::Udp,
			Transport::Tcp => ProtocolConfig::Tcp,
			Transport::Tls => ProtocolConfig::Tls {
				server_name: self.server_name(ip),
			},
			Transport::Https => ProtocolConfig::Https {
				server_name: self.server_name(ip),
				path: self
					.path
					.clone()
					.unwrap_or_else(|| Arc::from(DEFAULT_DNS_QUERY_PATH)),
			},
			Transport::Quic => ProtocolConfig::Quic {
				server_name: self.server_name(ip),
			},
			Transport::H3 => ProtocolConfig::H3 {
				server_name: self.server_name(ip),
				path: self
					.path
					.clone()
					.unwrap_or_else(|| Arc::from(DEFAULT_DNS_QUERY_PATH)),
				disable_grease: false,
			},
		};

		let mut connection = ConnectionConfig::new(protocol);
		connection.port = self.port;
		NameServerConfig::new(ip, true, vec![connection])
	}
}

/// How a server in `resolvers()` came to be reached the way it is (spec:OBS#resolvers).
#[derive(Clone, Copy, Debug)]
pub enum ResolverSource {
	/// Named in `dns.servers` by the caller.
	Configured,
	/// Discovered from the system's resolver configuration.
	Conventional,
}

impl ResolverSource {
	fn label(self) -> &'static str {
		match self {
			Self::Configured => "configured",
			Self::Conventional => "conventional",
		}
	}
}

/// One line of `resolvers()`: a server's address, the transport in use, and how it was arrived at.
#[derive(Clone, Debug)]
pub struct ResolverReport {
	pub address: String,
	pub transport: String,
	pub source: String,
}

/// `dns.maxStale`'s default: how far past expiry an answer may still be served.
///
/// An hour is long enough that a resolver outage does not stop an agent reaching hosts it already
/// knows, and short enough that a host which really has moved stops being served a dead address for
/// the life of a long-running process. The recovery path bounds the cost of being wrong to one
/// re-resolve, so the window can be generous (spec:DNS#serving-stale-answers).
pub const DEFAULT_MAX_STALE: Duration = Duration::from_secs(3600);

/// Everything `dns.*` configures about Faith's resolver, resolved from options at construction.
#[derive(Clone, Debug)]
pub struct ResolverSettings {
	/// The `dns.servers` list, in order. Empty means system discovery.
	pub servers: Vec<ServerSpec>,
	/// `dns.timeout`, bounding the whole list. `None` leaves hickory's five-second default.
	pub timeout: Option<Duration>,
	/// `dns.ndots`.
	pub ndots: Option<usize>,
	/// `dns.searchDomains`, replacing the system's search list when set.
	pub search_domains: Option<Vec<Name>>,
	/// `dns.hostsFile`: `Some(true)`/`Some(false)` force it on/off, `None` follows the platform.
	pub hosts_file: Option<bool>,
	/// `dns.exemptDomains`, added to the always-exempt `localhost`, `.local`, and system suffix.
	pub exempt_domains: Vec<Name>,
	/// `dns.serveStale`: whether an expired answer is served while a refresh runs behind it.
	pub serve_stale: bool,
	/// `dns.maxStale`: how far past expiry an answer may still be served.
	pub max_stale: Duration,
}

impl Default for ResolverSettings {
	fn default() -> Self {
		Self {
			servers: Vec::new(),
			timeout: None,
			ndots: None,
			search_domains: None,
			hosts_file: None,
			exempt_domains: Vec::new(),
			// Defaulted here as well as in the option parsing, so a resolver built directly (in tests,
			// and for the global default agent) serves stale like a configured one.
			serve_stale: true,
			max_stale: DEFAULT_MAX_STALE,
		}
	}
}

/// How many names the stale cache holds before evicting the least recently used.
///
/// Matched to hickory's own default answer-cache size, since the two hold an entry for the same set
/// of names: a stale entry only earns its place while hickory still plausibly holds, or recently
/// held, the answer it came from. Evicting one early costs a blocking lookup rather than a wrong
/// answer, so the bound is about memory rather than correctness.
const STALE_CACHE_SIZE: u64 = 8_192;

/// The resolver and the report of its servers, built together the first time the resolver is used.
struct Built {
	resolver: TokioResolver,
	reports: Vec<ResolverReport>,
}

/// A resolved answer kept past its TTL, so an expired lookup is served from it while a refresh runs
/// behind (spec:DNS#serving-stale-answers).
#[derive(Clone)]
struct StaleEntry {
	/// Shared rather than cloned per hit: a hit reads it and hands out a copy of the addresses.
	addrs: Arc<Vec<IpAddr>>,
	/// When the answer stopped being fresh, taken from the lookup rather than computed, so it is the
	/// TTL the resolver actually gave.
	valid_until: Instant,
}

/// Everything the resolver reads off the network, held together so a network change can drop it in
/// one go (spec:NETCHG). Each field describes the network the agent was on when it was read: which
/// servers discovery found, which suffixes are local to it, and which of its servers answered an
/// encryption probe. The caller's [`ResolverSettings`] deliberately sit outside, being options the
/// agent was constructed with rather than a reading of any network.
struct Generation {
	/// The configured (or discovered) resolver, built lazily inside a tokio runtime.
	built: OnceCell<Arc<Built>>,
	/// The system resolver, used for exempt names. Built lazily and independently.
	system: OnceCell<Arc<TokioResolver>>,
	/// The exempt suffixes, including the system's own, computed once per generation.
	exempt: OnceCell<Arc<Vec<Name>>>,
	/// Answers held past their TTL, keyed by the host as looked up. Sits in the generation rather
	/// than beside the settings so a network change drops it along with the resolvers that produced
	/// it: an address learned on the old network is exactly what must not be served on the new one.
	stale: moka::sync::Cache<String, StaleEntry>,
	/// Hosts with a refresh already in flight, so a second stale hit serves the entry rather than
	/// starting another lookup (spec:DNS#serving-stale-answers).
	refreshing: Mutex<HashSet<String>>,
}

impl Default for Generation {
	fn default() -> Self {
		Self {
			built: OnceCell::new(),
			system: OnceCell::new(),
			exempt: OnceCell::new(),
			stale: moka::sync::Cache::new(STALE_CACHE_SIZE),
			refreshing: Mutex::new(HashSet::new()),
		}
	}
}

struct Inner {
	/// The options the agent was constructed with. A network change does not touch these
	/// (spec:NETCHG#what-the-signal-keeps); they are what the next generation is rebuilt from.
	settings: ResolverSettings,
	/// Replaced wholesale by [`FaithResolver::reset`]. Read once at the start of a lookup rather
	/// than at each step, so a lookup that spans the signal finishes against the one set of
	/// resolvers it started on (spec:NETCHG#in-flight-requests).
	generation: Mutex<Arc<Generation>>,
}

/// A hickory resolver Faith owns, shared between reqwest's request path and `prefetchDns`.
#[derive(Clone)]
pub struct FaithResolver {
	inner: Arc<Inner>,
}

impl Default for FaithResolver {
	fn default() -> Self {
		Self::new(ResolverSettings::default())
	}
}

impl std::fmt::Debug for FaithResolver {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("FaithResolver").finish_non_exhaustive()
	}
}

impl FaithResolver {
	pub fn new(settings: ResolverSettings) -> Self {
		Self {
			inner: Arc::new(Inner {
				settings,
				generation: Mutex::new(Arc::new(Generation::default())),
			}),
		}
	}

	/// The generation a piece of work resolves against. Taken once per lookup: a reset swaps the
	/// generation rather than mutating it, so work already holding one carries on against the
	/// resolvers it started with (spec:NETCHG#in-flight-requests).
	fn generation(&self) -> Arc<Generation> {
		self.inner
			.generation
			.lock()
			.expect("the DNS generation lock is only held to clone or replace an Arc")
			.clone()
	}

	async fn built(&self, generation: &Generation) -> Result<Arc<Built>, NetError> {
		generation
			.built
			.get_or_try_init(|| async { build(&self.inner.settings).await.map(Arc::new) })
			.await
			.cloned()
	}

	/// The system resolver, for exempt names. Reads the system configuration and races both
	/// address families for Happy Eyeballs like the built-in resolver does.
	async fn system(&self, generation: &Generation) -> Result<Arc<TokioResolver>, NetError> {
		generation
			.system
			.get_or_try_init(|| async {
				let mut builder = TokioResolver::builder_tokio().unwrap_or_else(|_| {
					TokioResolver::builder_with_config(
						ResolverConfig::udp_and_tcp(&GOOGLE),
						TokioRuntimeProvider::default(),
					)
				});
				builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
				builder.build().map(Arc::new)
			})
			.await
			.cloned()
	}

	/// The exempt suffixes: `localhost`, `local`, the system's own domain and search suffixes, and
	/// the caller's `dns.exemptDomains` (spec:DNS#exempt-names). The system's own suffixes are a
	/// property of the network, so they are read per generation rather than once per agent.
	async fn exempt(&self, generation: &Generation) -> Arc<Vec<Name>> {
		generation
			.exempt
			.get_or_init(|| async {
				let system = read_system_conf()
					.map(|(config, _)| {
						config
							.domain()
							.into_iter()
							.chain(config.search())
							.cloned()
							.collect::<Vec<_>>()
					})
					.unwrap_or_default();
				Arc::new(exempt_suffixes(system, &self.inner.settings.exempt_domains))
			})
			.await
			.clone()
	}

	/// Whether `host` must go to the system resolver rather than Faith's servers.
	async fn is_exempt(&self, generation: &Generation, host: &str) -> bool {
		let Ok(name) = Name::from_utf8(host) else {
			return false;
		};
		self.exempt(generation)
			.await
			.iter()
			.any(|suffix| suffix.zone_of(&name))
	}

	/// Resolve `host` to its addresses, routing exempt names to the system resolver.
	async fn lookup(&self, host: &str) -> Result<Vec<IpAddr>, NetError> {
		let generation = self.generation();
		if self.is_exempt(&generation, host).await {
			// The system resolver keeps no cache Faith can hold answers in, so an exempt name has
			// nothing to go stale and is always resolved for real (spec:DNS#serving-stale-answers).
			let resolver = self.system(&generation).await?;
			return Ok(resolver.lookup_ip(host).await?.iter().collect());
		}

		if let Some(addrs) = self.stale_addrs(&generation, host) {
			self.spawn_refresh(&generation, host);
			return Ok(addrs);
		}

		let built = self.built(&generation).await?;
		let lookup = built.resolver.lookup_ip(host).await?;
		let addrs: Vec<IpAddr> = lookup.iter().collect();
		self.remember(&generation, host, &addrs, lookup.valid_until());
		Ok(addrs)
	}

	/// The addresses to serve for `host` without waiting, when its answer has expired but is still
	/// inside `dns.maxStale`.
	///
	/// `None` covers the three cases that must go to the resolver: no entry at all, an entry still
	/// fresh (which hickory's own cache answers without a network round trip anyway), and an entry so
	/// old it has stopped being evidence about the host.
	fn stale_addrs(&self, generation: &Generation, host: &str) -> Option<Vec<IpAddr>> {
		if !self.inner.settings.serve_stale {
			return None;
		}
		let entry = generation.stale.get(host)?;
		let now = Instant::now();
		if now <= entry.valid_until {
			return None;
		}
		if now.saturating_duration_since(entry.valid_until) > self.inner.settings.max_stale {
			// Dropped rather than left to sit: keeping it would let a refresh that has been failing
			// for hours go on being consulted, and the entry can only get older from here.
			generation.stale.invalidate(host);
			return None;
		}
		Some(entry.addrs.as_ref().clone())
	}

	/// Keep a successful answer for `host`, so a later lookup past its TTL has something to serve.
	fn remember(&self, generation: &Generation, host: &str, addrs: &[IpAddr], valid_until: Instant) {
		if !self.inner.settings.serve_stale || addrs.is_empty() {
			return;
		}
		generation.stale.insert(
			host.to_owned(),
			StaleEntry {
				addrs: Arc::new(addrs.to_vec()),
				valid_until,
			},
		);
	}

	/// Refresh `host` behind a stale answer that has already been served.
	///
	/// Single-flighted per host: the claim is taken before the task is spawned, so concurrent stale
	/// hits serve the entry rather than each starting a lookup. The task outlives the request that
	/// triggered it, and its outcome belongs to the cache rather than that request, so nothing here
	/// is reported to a caller (spec:DNS#serving-stale-answers).
	fn spawn_refresh(&self, generation: &Arc<Generation>, host: &str) {
		{
			let mut refreshing = generation
				.refreshing
				.lock()
				.expect("the DNS refresh lock is only held to insert or remove a host");
			if !refreshing.insert(host.to_owned()) {
				return;
			}
		}

		let this = self.clone();
		let generation = Arc::clone(generation);
		let host = host.to_owned();
		tokio::spawn(async move {
			match this.refresh(&generation, &host).await {
				Ok(()) => {}
				Err(err) if is_authoritatively_empty(&err) => {
					// The name resolves to nothing now, so the old address is not a stale answer for
					// it any more but a wrong one. Dropping the entry makes the next lookup fail
					// rather than hand out an address the host no longer answers on.
					generation.stale.invalidate(&host);
				}
				Err(_) => {
					// A network error, a server failure, or a timeout says nothing about where the
					// host is, so the entry stays and can be served again while this persists.
				}
			}
			generation
				.refreshing
				.lock()
				.expect("the DNS refresh lock is only held to insert or remove a host")
				.remove(&host);
		});
	}

	/// One refresh lookup, replacing the stale entry when it resolves.
	async fn refresh(&self, generation: &Generation, host: &str) -> Result<(), NetError> {
		let built = self.built(generation).await?;
		let lookup = built.resolver.lookup_ip(host).await?;
		let addrs: Vec<IpAddr> = lookup.iter().collect();
		self.remember(generation, host, &addrs, lookup.valid_until());
		Ok(())
	}

	/// Drop any stale answer held for `host`, so the next lookup waits for a fresh one.
	///
	/// Called when connecting to a served address failed, which is the one piece of evidence that the
	/// address was wrong rather than merely old (spec:DNS#when-a-stale-address-is-wrong).
	pub fn invalidate_stale(&self, host: &str) {
		self.generation().stale.invalidate(host);
	}

	/// Whether a lookup of `host` right now would be served from an expired entry, and so would hand
	/// out an address that is assumed rather than confirmed.
	///
	/// Deliberately the same window [`Self::stale_addrs`] serves from, rather than merely "an expired
	/// entry exists": an entry past `dns.maxStale` is resolved for real, and treating that as stale
	/// would spend a second connection attempt on an address that was already confirmed.
	pub fn served_stale(&self, host: &str) -> bool {
		if !self.inner.settings.serve_stale {
			return false;
		}
		self.generation().stale.get(host).is_some_and(|entry| {
			let now = Instant::now();
			now > entry.valid_until
				&& now.saturating_duration_since(entry.valid_until) <= self.inner.settings.max_stale
		})
	}

	/// Resolve `host` and leave the answer in the shared cache, so a later request skips the
	/// lookup. Any failure is swallowed: the warm-up is advisory (spec:WARM).
	pub async fn prefetch(&self, host: &str) {
		let _ = self.lookup(host).await;
	}

	/// The DNS servers the agent resolves through, in query order (spec:OBS#resolvers). Empty
	/// until the resolver has been used, because it reads its configuration on first use.
	pub fn resolvers(&self) -> Vec<ResolverReport> {
		self.generation()
			.built
			.get()
			.map(|built| built.reports.clone())
			.unwrap_or_default()
	}

	/// Drop everything read off the network, so the next lookup rebuilds against the network the
	/// agent is on now.
	///
	/// Flushing cached answers alone would leave the agent resolving them again through the
	/// previous network's servers: the discovered server list, the suffixes treated as local, and
	/// the results of encryption probes are all readings of a network too, and the whole point of
	/// the signal is that the network has changed. Dropping the generation takes the caches with
	/// it, since they belong to the resolvers being dropped.
	///
	/// The caller's options are untouched, so a listed `dns.servers` set is rebuilt exactly as
	/// configured; what it re-reads is what the system supplies and what the network answers
	/// (spec:NETCHG#what-the-signal-keeps).
	///
	/// Synchronous, unlike the rest of this type: it swaps an `Arc` rather than building anything,
	/// which keeps it callable from `networkChanged`, which is not async. Nothing is rebuilt here
	/// either, so an agent that never resolves again pays nothing for the signal.
	///
	/// spec:NETCHG#reach-across-the-subsystems
	pub fn reset(&self) {
		*self
			.inner
			.generation
			.lock()
			.expect("the DNS generation lock is only held to clone or replace an Arc") =
			Arc::new(Generation::default());
	}
}

impl Resolve for FaithResolver {
	fn resolve(&self, name: ReqName) -> Resolving {
		let this = self.clone();
		Box::pin(async move {
			let addrs = this.lookup(name.as_str()).await?;
			// Port `0` is a placeholder reqwest fills from the URL. The returned `Addrs` has to be
			// `'static`, so collect owned rather than borrowing the lookup.
			let addrs: Vec<SocketAddr> =
				addrs.into_iter().map(|ip| SocketAddr::new(ip, 0)).collect();
			Ok(Box::new(addrs.into_iter()) as Addrs)
		})
	}
}

/// Whether a failed lookup was the resolver answering that the name holds nothing, rather than the
/// resolver failing to answer.
///
/// The distinction decides what happens to a stale entry: an authoritative "nothing here" retires
/// it, while a failure to reach an answer leaves it in place. Hickory draws the same line, producing
/// `NoRecordsFound` only for `NXDOMAIN` and for `NOERROR` with no answer records, and reporting
/// `SERVFAIL` and the other failure codes as `ResponseCode` instead.
fn is_authoritatively_empty(err: &NetError) -> bool {
	matches!(err, NetError::Dns(DnsError::NoRecordsFound(_)))
}

/// Apply the options common to every resolver Faith builds: race both families for Happy Eyeballs,
/// hold the caller's order fixed rather than reordering by latency, and layer any `dns.*` timeout,
/// ndots, and hosts-file settings on top.
fn apply_options(
	builder: &mut hickory_resolver::ResolverBuilder<TokioRuntimeProvider>,
	settings: &ResolverSettings,
) {
	let options = builder.options_mut();
	options.ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
	// The list expresses the caller's intent, not a performance hint, so a private resolver named
	// first must not lose traffic to a closer fallback (spec:DNS#server-order).
	options.server_ordering_strategy = ServerOrderingStrategy::UserProvidedOrder;
	if let Some(timeout) = settings.timeout {
		options.timeout = timeout;
	}
	if let Some(ndots) = settings.ndots {
		options.ndots = ndots;
	}
	if let Some(hosts_file) = settings.hosts_file {
		options.use_hosts_file = if hosts_file {
			ResolveHosts::Always
		} else {
			ResolveHosts::Never
		};
	}
}

/// Build the configured (or discovered) resolver and the report of its servers.
async fn build(settings: &ResolverSettings) -> Result<Built, NetError> {
	if settings.servers.is_empty() {
		build_discovery(settings)
	} else {
		build_listed(settings).await
	}
}

/// Discovery: configure from the system, then let hickory's RFC 9539 opportunistic encryption
/// upgrade those servers to DoT/DoQ where they answer a probe. `dns.searchDomains` overrides the
/// system search list when set (spec:DNS#discovery).
fn build_discovery(settings: &ResolverSettings) -> Result<Built, NetError> {
	let (mut config, options) = read_system_conf().unwrap_or_else(|_| {
		// A host with no readable resolver configuration falls back to Google Public DNS over
		// conventional DNS, probed like any other server (spec:DNS#discovery).
		(
			ResolverConfig::udp_and_tcp(&GOOGLE),
			hickory_resolver::config::ResolverOpts::default(),
		)
	});

	if let Some(search) = &settings.search_domains {
		config = ResolverConfig::from_parts(None, search.clone(), config.name_servers().to_vec());
	}

	let reports = report(config.name_servers(), ResolverSource::Conventional);

	let mut builder = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
		.with_options(options);
	apply_options(&mut builder, settings);
	let builder = builder.with_opportunistic_encryption(OpportunisticEncryption::Enabled {
		config: Default::default(),
	});

	Ok(Built {
		resolver: builder.build()?,
		reports,
	})
}

/// The listed-servers path: bootstrap any hostname hosts to addresses, then build the resolver
/// from the parsed specs in order (spec:DNS#transports, spec:DNS#bootstrapping).
async fn build_listed(settings: &ResolverSettings) -> Result<Built, NetError> {
	let name_servers = build_name_servers(settings).await?;

	let search = settings.search_domains.clone().unwrap_or_default();
	let config = ResolverConfig::from_parts(None, search, name_servers.clone());
	let reports = report(&name_servers, ResolverSource::Configured);

	let mut builder = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default());
	apply_options(&mut builder, settings);

	Ok(Built {
		resolver: builder.build()?,
		reports,
	})
}

/// Resolve the listed servers to hickory name servers, bootstrapping hostname hosts. A hostname
/// that will not resolve drops that server for the life of the agent rather than failing the
/// resolver (spec:DNS#bootstrapping).
async fn build_name_servers(
	settings: &ResolverSettings,
) -> Result<Vec<NameServerConfig>, NetError> {
	let needs_bootstrap = settings.servers.iter().any(|spec| spec.ip().is_none());
	let bootstrap = if needs_bootstrap {
		Some(bootstrap_resolver(settings)?)
	} else {
		None
	};

	let mut name_servers = Vec::with_capacity(settings.servers.len());
	for spec in &settings.servers {
		let ip = match spec.ip() {
			Some(ip) => ip,
			None => {
				let ServerHost::Name(host) = &spec.host else {
					unreachable!("ip() is None only for a hostname host");
				};
				let resolver = bootstrap
					.as_ref()
					.expect("bootstrap resolver built when a hostname host is present");
				match resolver.lookup_ip(host.as_str()).await {
					Ok(lookup) => match lookup.iter().next() {
						Some(ip) => ip,
						None => continue,
					},
					// The hostname does not resolve: drop this server for the agent's life.
					Err(_) => continue,
				}
			}
		};
		name_servers.push(spec.to_name_server(ip));
	}

	Ok(name_servers)
}

/// The resolver that bootstraps hostname servers: the listed IP-host servers in order, so an
/// encrypted server placed first resolves its siblings without exposing the hostname in plaintext.
/// Where the list names no IP host, the system's own configuration bootstraps instead.
fn bootstrap_resolver(settings: &ResolverSettings) -> Result<TokioResolver, NetError> {
	let ip_servers: Vec<NameServerConfig> = settings
		.servers
		.iter()
		.filter_map(|spec| spec.ip().map(|ip| spec.to_name_server(ip)))
		.collect();

	let mut builder = if ip_servers.is_empty() {
		TokioResolver::builder_tokio().unwrap_or_else(|_| {
			TokioResolver::builder_with_config(
				ResolverConfig::udp_and_tcp(&GOOGLE),
				TokioRuntimeProvider::default(),
			)
		})
	} else {
		TokioResolver::builder_with_config(
			ResolverConfig::from_parts(None, vec![], ip_servers),
			TokioRuntimeProvider::default(),
		)
	};
	builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
	builder.options_mut().server_ordering_strategy = ServerOrderingStrategy::UserProvidedOrder;
	builder.build()
}

/// The suffixes handed to the system resolver rather than Faith's servers: `localhost` and `local`
/// always, plus the ones the system supplies and the caller's `dns.exemptDomains`
/// (spec:DNS#exempt-names).
///
/// The root name is never a suffix here, whichever list it arrives in. It is the parent of every
/// name, so admitting it would exempt the lot and route every lookup to the system resolver with
/// `dns.servers` configured and unused. It does arrive in practice: a Windows host with no DNS
/// domain of its own reports the root as its domain, so the check is what keeps the encrypted
/// transports working there rather than being quietly bypassed.
fn exempt_suffixes(system: Vec<Name>, configured: &[Name]) -> Vec<Name> {
	let mut names = vec![
		Name::from_ascii("localhost").unwrap(),
		Name::from_ascii("local").unwrap(),
	];
	names.extend(
		system
			.into_iter()
			.chain(configured.iter().cloned())
			.filter(|name| !name.is_root()),
	);
	names
}

/// Summarise name servers for `resolvers()`, in the order they are queried.
fn report(name_servers: &[NameServerConfig], source: ResolverSource) -> Vec<ResolverReport> {
	let mut reports = Vec::new();
	for server in name_servers {
		for connection in &server.connections {
			let transport = match connection.protocol {
				ProtocolConfig::Udp => Transport::Udp,
				ProtocolConfig::Tcp => Transport::Tcp,
				ProtocolConfig::Tls { .. } => Transport::Tls,
				ProtocolConfig::Https { .. } => Transport::Https,
				ProtocolConfig::Quic { .. } => Transport::Quic,
				ProtocolConfig::H3 { .. } => Transport::H3,
			};
			reports.push(ResolverReport {
				address: SocketAddr::new(server.ip, connection.port).to_string(),
				transport: transport.label().to_owned(),
				source: source.label().to_owned(),
			});
		}
	}
	reports
}

#[cfg(test)]
mod tests {
	use super::*;

	fn spec(input: &str) -> ServerSpec {
		ServerSpec::parse(input).expect("valid server URL")
	}

	#[test]
	fn scheme_selects_transport_and_conventional_port() {
		// spec:DNS#transports
		assert_eq!(spec("udp://1.1.1.1").transport, Transport::Udp);
		assert_eq!(spec("udp://1.1.1.1").port, 53);
		assert_eq!(spec("tcp://1.1.1.1").port, 53);
		assert_eq!(spec("tls://1.1.1.1").transport, Transport::Tls);
		assert_eq!(spec("tls://1.1.1.1").port, 853);
		assert_eq!(spec("quic://1.1.1.1").port, 853);
		assert_eq!(spec("https://1.1.1.1").transport, Transport::Https);
		assert_eq!(spec("https://1.1.1.1").port, 443);
		assert_eq!(spec("h3://1.1.1.1").port, 443);
	}

	#[test]
	fn explicit_port_overrides_the_conventional_one() {
		// spec:DNS#transports
		assert_eq!(spec("tls://1.1.1.1:8853").port, 8853);
	}

	#[test]
	fn http_transports_default_the_query_path() {
		// spec:DNS#transports — `/dns-query` when the URL supplies none.
		assert_eq!(spec("https://dns.google").path, None);
		assert_eq!(
			spec("https://dns.google")
				.to_name_server(IpAddr::from([8, 8, 8, 8]))
				.connections[0]
				.protocol,
			ProtocolConfig::Https {
				server_name: Arc::from("dns.google"),
				path: Arc::from(DEFAULT_DNS_QUERY_PATH),
			}
		);
		assert_eq!(
			spec("https://dns.google/resolve").path,
			Some(Arc::from("/resolve"))
		);
	}

	#[test]
	fn a_fragment_names_the_certificate() {
		// spec:DNS#transports — `tls://1.1.1.1#cloudflare-dns.com`.
		let spec = spec("tls://1.1.1.1#cloudflare-dns.com");
		assert_eq!(spec.cert_name.as_deref(), Some("cloudflare-dns.com"));
		assert_eq!(
			&*spec.server_name(IpAddr::from([1, 1, 1, 1])),
			"cloudflare-dns.com"
		);
	}

	#[test]
	fn a_bare_ip_authenticates_against_the_address() {
		// spec:DNS#transports — `tls://1.1.1.1` with no fragment.
		let spec = spec("tls://1.1.1.1");
		assert_eq!(spec.cert_name, None);
		assert_eq!(&*spec.server_name(IpAddr::from([1, 1, 1, 1])), "1.1.1.1");
	}

	#[test]
	fn a_hostname_authenticates_against_itself() {
		// spec:DNS#transports
		let spec = spec("tls://dns.google");
		assert_eq!(&*spec.server_name(IpAddr::from([8, 8, 8, 8])), "dns.google");
	}

	#[test]
	fn an_unknown_scheme_is_rejected() {
		// spec:DNS#transports — throws an address-parse error at construction.
		assert!(ServerSpec::parse("ftp://1.1.1.1").is_err());
		assert!(ServerSpec::parse("not a url").is_err());
	}

	#[tokio::test]
	async fn reset_replaces_the_generation_and_what_it_holds() {
		// A network change drops what was read off the old network, so the next lookup builds
		// against the new one rather than reusing the previous network's servers (spec:NETCHG).
		let resolver = FaithResolver::new(ResolverSettings {
			servers: vec![spec("udp://127.0.0.1:1")],
			timeout: Some(Duration::from_millis(200)),
			..ResolverSettings::default()
		});

		let before = resolver.generation();
		// Build the generation's state, so there is something for the reset to drop.
		let _ = resolver.built(&before).await;
		assert!(
			before.built.get().is_some(),
			"the generation built its resolver"
		);
		assert_eq!(resolver.resolvers().len(), 1, "which `resolvers()` reports");

		resolver.reset();

		let after = resolver.generation();
		assert!(
			!Arc::ptr_eq(&before, &after),
			"the reset swaps the generation rather than mutating it"
		);
		assert!(
			after.built.get().is_none(),
			"the new generation holds nothing until it is used again"
		);
		assert!(
			before.built.get().is_some(),
			"work already holding the old generation keeps its resolvers"
		);
		assert!(
			resolver.resolvers().is_empty(),
			"`resolvers()` reports nothing until the rebuild (spec:OBS#resolvers)"
		);

		// Configuration survives the signal, so the rebuild uses the servers as configured.
		let _ = resolver.built(&after).await;
		assert_eq!(
			resolver.resolvers().len(),
			1,
			"the rebuilt generation resolves through the configured servers again"
		);
	}

	/// A resolver with a stale entry for `host` whose freshness ended `ago`.
	fn with_stale_entry(settings: ResolverSettings, host: &str, ago: Duration) -> FaithResolver {
		let resolver = FaithResolver::new(settings);
		resolver.generation().stale.insert(
			host.to_owned(),
			StaleEntry {
				addrs: Arc::new(vec![IpAddr::from([127, 0, 0, 1])]),
				valid_until: Instant::now() - ago,
			},
		);
		resolver
	}

	#[test]
	fn only_an_expired_entry_inside_the_window_is_served_stale() {
		// spec:DNS#serving-stale-answers
		let settings = || ResolverSettings {
			max_stale: Duration::from_secs(60),
			..ResolverSettings::default()
		};

		// Still fresh: the lookup goes through hickory, which answers from its own cache.
		let fresh = FaithResolver::new(settings());
		fresh.generation().stale.insert(
			"fresh.test".to_owned(),
			StaleEntry {
				addrs: Arc::new(vec![IpAddr::from([127, 0, 0, 1])]),
				valid_until: Instant::now() + Duration::from_secs(60),
			},
		);
		let generation = fresh.generation();
		assert!(
			fresh.stale_addrs(&generation, "fresh.test").is_none(),
			"a fresh entry is not a stale hit"
		);

		// Expired but inside `dns.maxStale`: served immediately.
		let stale = with_stale_entry(settings(), "stale.test", Duration::from_secs(5));
		let generation = stale.generation();
		assert!(
			stale.stale_addrs(&generation, "stale.test").is_some(),
			"an entry expired inside the window is served"
		);

		// Past the window: no longer evidence about the host, so the lookup blocks.
		let old = with_stale_entry(settings(), "old.test", Duration::from_secs(120));
		let generation = old.generation();
		assert!(
			old.stale_addrs(&generation, "old.test").is_none(),
			"an entry past `dns.maxStale` is not served"
		);
		assert!(
			generation.stale.get("old.test").is_none(),
			"and is dropped rather than left to age further"
		);
	}

	#[test]
	fn serve_stale_off_never_serves_an_expired_entry() {
		// spec:DNS#serving-stale-answers — the switch for a caller that must not connect to an
		// address it knows to be out of date.
		let resolver = with_stale_entry(
			ResolverSettings {
				serve_stale: false,
				..ResolverSettings::default()
			},
			"strict.test",
			Duration::from_secs(5),
		);
		let generation = resolver.generation();
		assert!(resolver.stale_addrs(&generation, "strict.test").is_none());
		assert!(
			!resolver.served_stale("strict.test"),
			"and nothing is reported as stale-served, so no retry is armed"
		);
	}

	#[test]
	fn served_stale_tracks_the_window_it_serves_from() {
		// The retry layer arms itself from this, so it must not claim an address was assumed when
		// the lookup actually blocked on a fresh one (spec:DNS#when-a-stale-address-is-wrong).
		let settings = || ResolverSettings {
			max_stale: Duration::from_secs(60),
			..ResolverSettings::default()
		};

		let inside = with_stale_entry(settings(), "inside.test", Duration::from_secs(5));
		assert!(inside.served_stale("inside.test"));

		let outside = with_stale_entry(settings(), "outside.test", Duration::from_secs(120));
		assert!(
			!outside.served_stale("outside.test"),
			"an entry past the window is resolved for real, so its address is confirmed"
		);

		let absent = FaithResolver::new(settings());
		assert!(!absent.served_stale("absent.test"));
	}

	#[test]
	fn a_network_change_drops_stale_answers() {
		// Addresses read off the old network are exactly what must not be served on the new one
		// (spec:NETCHG#reach-across-the-subsystems).
		let resolver = with_stale_entry(
			ResolverSettings::default(),
			"netchg.test",
			Duration::from_secs(5),
		);
		assert!(resolver.served_stale("netchg.test"));

		resolver.reset();

		assert!(
			!resolver.served_stale("netchg.test"),
			"the stale answer goes with the generation that held it"
		);
	}

	#[test]
	fn a_root_suffix_never_exempts_everything() {
		// A Windows host with no DNS domain reports the root as its domain, and the root is the
		// parent of every name. Taking it as a suffix exempted every lookup and sent it to the
		// system resolver, leaving `dns.servers` configured and unused (spec:DNS#exempt-names).
		let suffixes = exempt_suffixes(vec![Name::root()], &[]);
		assert!(
			!suffixes.iter().any(|suffix| suffix.is_root()),
			"the root is not admitted as a suffix"
		);

		let name = Name::from_utf8("nonexistent.example").unwrap();
		assert!(
			!suffixes.iter().any(|suffix| suffix.zone_of(&name)),
			"so an ordinary name is not exempt and reaches the configured servers"
		);

		// The names that must stay exempt still are, and a real system suffix still counts.
		let suffixes = exempt_suffixes(
			vec![Name::root(), Name::from_utf8("corp.example").unwrap()],
			&[],
		);
		for exempt in ["localhost", "printer.local", "host.corp.example"] {
			let name = Name::from_utf8(exempt).unwrap();
			assert!(
				suffixes.iter().any(|suffix| suffix.zone_of(&name)),
				"{exempt} is exempt"
			);
		}
	}

	#[test]
	fn a_root_entry_from_the_caller_is_refused_too() {
		// Whichever list it arrives in, the root would disable the caller's own servers.
		let suffixes = exempt_suffixes(vec![], &[Name::root()]);
		let name = Name::from_utf8("nonexistent.example").unwrap();
		assert!(!suffixes.iter().any(|suffix| suffix.zone_of(&name)));
	}

	#[test]
	fn exempt_matches_a_suffix_exactly_or_as_a_subdomain() {
		// spec:DNS#exempt-names
		let local = Name::from_ascii("local").unwrap();
		assert!(local.zone_of(&Name::from_utf8("printer.local").unwrap()));
		assert!(local.zone_of(&Name::from_utf8("local").unwrap()));
		assert!(!local.zone_of(&Name::from_utf8("mylocal.example").unwrap()));
	}
}
