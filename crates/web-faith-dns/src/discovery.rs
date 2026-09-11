//! Resolver discovery from the system configuration.
use hickory_resolver::{
	TokioResolver,
	config::{
		GOOGLE, LookupIpStrategy, NameServerConfig, OpportunisticEncryption, ResolveHosts,
		ResolverConfig, ServerOrderingStrategy,
	},
	net::{NetError, runtime::TokioRuntimeProvider},
	system_conf::read_system_conf,
};

use crate::{
	settings::{ResolverReport, ResolverSettings, ResolverSource, report},
	transport::ServerHost,
};

/// The resolver and the report of its servers, built together the first time the resolver is used.
pub(crate) struct Built {
	pub(crate) resolver: TokioResolver,
	pub(crate) reports: Vec<ResolverReport>,
}

/// Apply the options common to every resolver Faith builds: race both families for Happy Eyeballs,
/// hold the caller's order fixed rather than reordering by latency, and layer any `dns.*` timeout,
/// ndots, and hosts-file settings on top.
pub(crate) fn apply_options(
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

/// Discovery: configure from the system, then let hickory's RFC 9539 opportunistic encryption
/// upgrade those servers to DoT/DoQ where they answer a probe. `dns.searchDomains` overrides the
/// system search list when set.
// spec:DNS#discovery
pub(crate) fn build_discovery(settings: &ResolverSettings) -> Result<Built, NetError> {
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

/// The resolver that bootstraps hostname servers: the listed IP-host servers in order, so an
/// encrypted server placed first resolves its siblings without exposing the hostname in plaintext.
/// Where the list has no IP host, the system's own configuration bootstraps instead.
pub(crate) fn bootstrap_resolver(settings: &ResolverSettings) -> Result<TokioResolver, NetError> {
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

/// Build the configured (or discovered) resolver and the report of its servers.
pub(crate) async fn build(settings: &ResolverSettings) -> Result<Built, NetError> {
	if settings.servers.is_empty() {
		build_discovery(settings)
	} else {
		build_listed(settings).await
	}
}

/// The listed-servers path: bootstrap any hostname hosts to addresses, then build the resolver
/// from the parsed specs in order.
// spec:DNS#transports
// spec:DNS#bootstrapping
pub(crate) async fn build_listed(settings: &ResolverSettings) -> Result<Built, NetError> {
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
/// resolver.
// spec:DNS#bootstrapping
pub(crate) async fn build_name_servers(
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
