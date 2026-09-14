//! Resolver settings.
use std::{fmt, net::SocketAddr, time::Duration};

use hickory_resolver::{
	config::{NameServerConfig, ProtocolConfig},
	proto::rr::Name,
};

use crate::transport::{ServerSpec, Transport};

/// How a nameserver came to be reached the way it is.
// spec:OBS#resolvers
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolverSource {
	/// Named by the caller.
	Configured,
	/// Discovered from the system's resolver configuration.
	Conventional,
}

impl fmt::Display for ResolverSource {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			Self::Configured => "configured",
			Self::Conventional => "conventional",
		})
	}
}

/// One line of `resolvers()`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ResolverReport {
	/// The nameserver's address.
	pub address: SocketAddr,
	/// The transport in use.
	pub transport: Transport,
	/// How that transport was arrived at.
	pub source: ResolverSource,
}

/// [`ResolverConfig::serve_stale`]'s default.
///
/// Long enough that a resolver outage does not stop an agent reaching hosts it knows, short enough
/// that a host which has moved stops being served a dead address for the life of the process.
// spec:DNS#serving-stale-answers
pub const DEFAULT_MAX_STALE: Duration = Duration::from_secs(3600);

/// Configuration for initialising a [`FaithResolver`](crate::FaithResolver).
#[derive(Clone, Debug)]
pub struct ResolverConfig {
	/// The nameservers to consult, in order. Empty takes the system's own configuration.
	pub servers: Vec<ServerSpec>,
	/// How long a lookup may take across the whole server list, so exhausting several dead
	/// servers costs one timeout rather than one each.
	pub timeout: Option<Duration>,
	/// How many dots a name must contain before it is tried as given, ahead of the search list.
	pub ndots: Option<usize>,
	/// The domains appended to a name that is not fully qualified, replacing the system's list.
	pub search_domains: Option<Vec<Name>>,
	/// Whether to consult the hosts file. `None` follows the platform's own convention.
	pub hosts_file: Option<bool>,
	/// Further domains to send to the system resolver, added to the ones always exempt.
	pub exempt_domains: Vec<Name>,
	/// Whether an expired answer may be served, and how long for.
	///
	/// A fresh lookup runs behind one that is; an entry older than this is discarded instead.
	pub serve_stale: Option<Duration>,
}

impl Default for ResolverConfig {
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
			serve_stale: Some(DEFAULT_MAX_STALE),
		}
	}
}

/// The suffixes handed to the system resolver rather than the configured ones: `localhost` and
/// `local` always, plus the system's own and the caller's.
///
/// The root name is never a suffix here, whichever list it arrives in. It is the parent of every
/// name, so admitting it would exempt the lot and route every lookup to the system resolver with
/// servers configured and unused. It does arrive in practice: a Windows host with no DNS
/// domain of its own reports the root as its domain, so the check keeps the encrypted
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
				address: SocketAddr::new(server.ip, connection.port),
				transport,
				source,
			});
		}
	}
	reports
}

#[cfg(test)]
mod tests;
