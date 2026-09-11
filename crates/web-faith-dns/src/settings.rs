//! Resolver settings.
use std::{net::SocketAddr, time::Duration};

use hickory_resolver::{
	config::{NameServerConfig, ProtocolConfig},
	proto::rr::Name,
};

use crate::transport::{ServerSpec, Transport};

/// How a server in `resolvers()` came to be reached the way it is.
// spec:OBS#resolvers
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
/// Long enough that a resolver outage does not stop an agent reaching hosts it knows, short enough
/// that a host which has moved stops being served a dead address for the life of the process.
// spec:DNS#serving-stale-answers
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

/// The suffixes handed to the system resolver rather than Faith's servers: `localhost` and `local`
/// always, plus the ones the system supplies and the caller's `dns.exemptDomains`.
///
/// The root name is never a suffix here, whichever list it arrives in. It is the parent of every
/// name, so admitting it would exempt the lot and route every lookup to the system resolver with
/// `dns.servers` configured and unused. It does arrive in practice: a Windows host with no DNS
/// domain of its own reports the root as its domain, so the check is what keeps the encrypted
/// transports working there rather than being quietly bypassed.
// spec:DNS#exempt-names
pub(crate) fn exempt_suffixes(system: Vec<Name>, configured: &[Name]) -> Vec<Name> {
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
pub(crate) fn report(
	name_servers: &[NameServerConfig],
	source: ResolverSource,
) -> Vec<ResolverReport> {
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
mod tests;
