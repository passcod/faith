use std::{
	collections::HashSet,
	net::IpAddr,
	sync::{Arc, Mutex},
	time::Instant,
};

use hickory_resolver::{
	TokioResolver,
	config::{GOOGLE, LookupIpStrategy, ResolverConfig},
	net::{DnsError, NetError, runtime::TokioRuntimeProvider},
	proto::rr::{Name, RecordType},
	system_conf::read_system_conf,
};
use tokio::sync::OnceCell;

#[cfg(feature = "reqwest")]
use std::net::SocketAddr;

use crate::{
	discovery::{Built, build},
	https::{HttpsSink, read_https_answer},
	settings::{ResolverReport, ResolverSettings, exempt_suffixes},
};

/// How many names the stale cache holds before evicting the least recently used.
///
/// Matched to hickory's own default answer-cache size, since the two hold an entry for the same set
/// of names: a stale entry only earns its place while hickory still plausibly holds, or recently
/// held, the answer it came from. Evicting one early costs a blocking lookup rather than a wrong
/// answer, so the bound is about memory rather than correctness.
const STALE_CACHE_SIZE: u64 = 8_192;

/// A resolved answer kept past its TTL, so an expired lookup is served from it while a refresh runs
/// behind.
// spec:DNS#serving-stale-answers
#[derive(Clone)]
struct StaleEntry {
	/// Shared rather than cloned per hit: a hit reads it and hands out a copy of the addresses.
	addrs: Arc<Vec<IpAddr>>,
	/// When the answer stopped being fresh, taken from the lookup rather than computed, so it is the
	/// TTL the resolver actually gave.
	valid_until: Instant,
}

/// Everything the resolver reads off the network, held together so a network change can drop it in
/// one go. Each field describes the network the agent was on when it was read: which
/// servers discovery found, which suffixes are local to it, and which of its servers answered an
/// encryption probe. The caller's [`ResolverSettings`] deliberately sit outside, being options the
/// agent was constructed with rather than a reading of any network.
// spec:NETCHG
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
	/// starting another lookup.
	// spec:DNS#serving-stale-answers
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
	/// The options the agent was constructed with. A network change does not touch these; they are
	/// what the next generation is rebuilt from.
	// spec:NETCHG#what-the-signal-keeps
	settings: ResolverSettings,
	/// Replaced wholesale by [`FaithResolver::reset`]. Read once at the start of a lookup rather
	/// than at each step, so a lookup that spans the signal finishes against the one set of
	/// resolvers it started on.
	// spec:NETCHG#in-flight-requests
	generation: Mutex<Arc<Generation>>,
	/// Where `HTTPS` records go, installed by the agent once the upgrade cache and prober exist.
	///
	/// Sits beside the settings rather than inside the generation deliberately: it is wiring
	/// rather than something read off a network, so a network change leaves it in place. Its
	/// absence is what turns the `HTTPS` query off, so an agent with HTTP/3 upgrade disabled, or
	/// one on the system resolver, never sends one.
	https_sink: Mutex<Option<Arc<dyn HttpsSink>>>,
}

/// A hickory resolver Faith owns, shared between a client's request path and [`FaithResolver::prefetch`].
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
				https_sink: Mutex::new(None),
			}),
		}
	}

	/// Install where `HTTPS` records go, enabling the query.
	///
	/// Called after the agent's HTTP/3 upgrade cache and prober are built, which cannot happen
	/// before the resolver exists. Replaces any previous sink, which is what a network change
	/// needs: the prober is rebuilt with the client, so the sink must be too or it would kick
	/// probes onto a client that has been dropped.
	// spec:DNS#https-records
	pub fn set_https_sink(&self, sink: Arc<dyn HttpsSink>) {
		*self
			.inner
			.https_sink
			.lock()
			.expect("the HTTPS sink lock is only held to clone or replace an Arc") = Some(sink);
	}

	fn https_sink(&self) -> Option<Arc<dyn HttpsSink>> {
		self.inner
			.https_sink
			.lock()
			.expect("the HTTPS sink lock is only held to clone or replace an Arc")
			.clone()
	}

	/// The generation a piece of work resolves against. Taken once per lookup: a reset swaps the
	/// generation rather than mutating it, so work already holding one carries on against the
	/// resolvers it started with.
	// spec:NETCHG#in-flight-requests
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
	/// the caller's `dns.exemptDomains`. The system's own suffixes are a
	/// property of the network, so they are read per generation rather than once per agent.
	// spec:DNS#exempt-names
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

		// Alongside the addresses rather than after them: the record is a hint for the upgrade
		// layer to verify, so nothing about connecting waits on it (spec:DNS#https-records).
		self.spawn_https_query(&generation, host);

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

	/// Ask for `host`'s `HTTPS` record behind the address lookup, so an origin advertising
	/// `alpn="h3"` is known before the first connection rather than after the first TCP response.
	///
	/// Spawned rather than awaited: an absent, slow, or failed answer must leave address
	/// resolution and connecting untouched. Its outcome belongs to the upgrade layer rather than
	/// to the request that triggered it, so nothing here reaches a caller, exactly as a stale
	/// refresh's outcome does not.
	// spec:DNS#https-records
	fn spawn_https_query(&self, generation: &Arc<Generation>, host: &str) {
		let Some(sink) = self.https_sink() else {
			return;
		};
		// Asked before the query, not after: an origin already confirmed, failed, or holding a
		// live advertisement has nothing to learn, so it costs no DNS traffic.
		if !sink.wants(host) {
			return;
		}
		let Ok(name) = Name::from_utf8(host) else {
			return;
		};

		let this = self.clone();
		let generation = Arc::clone(generation);
		let host = host.to_owned();
		tokio::spawn(async move {
			let Ok(built) = this.built(&generation).await else {
				return;
			};
			let Ok(lookup) = built.resolver.lookup(name, RecordType::HTTPS).await else {
				return;
			};
			// The answer's own query name, not the host as written: the search list may have
			// requalified it, and the record's target is judged against what was actually asked.
			if let Some(advertisement) = read_https_answer(lookup.query().name(), lookup.answers())
			{
				sink.record(&host, advertisement);
			}
		});
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
	fn remember(
		&self,
		generation: &Generation,
		host: &str,
		addrs: &[IpAddr],
		valid_until: Instant,
	) {
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
	/// is reported to a caller.
	// spec:DNS#serving-stale-answers
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
	/// address was wrong rather than merely old.
	// spec:DNS#when-a-stale-address-is-wrong
	pub fn invalidate_stale(&self, host: &str) {
		self.generation().stale.invalidate(host);
	}

	/// Whether a lookup of `host` right now would be served from an expired entry, and so would hand
	/// out an address that is assumed rather than confirmed.
	///
	/// Deliberately the same window a stale answer is served from, rather than merely "an expired
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
	/// lookup. Any failure is swallowed: the warm-up is advisory.
	// spec:WARM
	pub async fn prefetch(&self, host: &str) {
		let _ = self.lookup(host).await;
	}

	/// The DNS servers the agent resolves through, in query order. Empty
	/// until the resolver has been used, because it reads its configuration on first use.
	// spec:OBS#resolvers
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
	/// configured; what it re-reads is what the system supplies and what the network answers.
	///
	/// Synchronous, unlike the rest of this type: it swaps an `Arc` rather than building anything,
	/// which keeps it callable from a network-change signal, which is not async. Nothing is rebuilt here
	/// either, so an agent that never resolves again pays nothing for the signal.
	// spec:NETCHG#what-the-signal-keeps
	// spec:NETCHG#reach-across-the-subsystems
	pub fn reset(&self) {
		*self
			.inner
			.generation
			.lock()
			.expect("the DNS generation lock is only held to clone or replace an Arc") =
			Arc::new(Generation::default());
	}
}

/// Installed on a reqwest client with `ClientBuilder::dns_resolver`, so every lookup a request
/// makes goes through the same resolver `prefetch` warms.
#[cfg(feature = "reqwest")]
impl reqwest::dns::Resolve for FaithResolver {
	fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
		let this = self.clone();
		Box::pin(async move {
			let addrs = this.lookup(name.as_str()).await?;
			// Port `0` is a placeholder reqwest fills from the URL. The returned `Addrs` has to be
			// `'static`, so collect owned rather than borrowing the lookup.
			let addrs: Vec<SocketAddr> =
				addrs.into_iter().map(|ip| SocketAddr::new(ip, 0)).collect();
			Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
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

#[cfg(test)]
mod tests;
