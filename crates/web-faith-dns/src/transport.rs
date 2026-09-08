use std::{net::IpAddr, sync::Arc};

use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ProtocolConfig};
use url::{Host, Url};

/// The default DoH/DoQ query path, used when a `https://`/`h3://` server URL supplies none.
const DEFAULT_DNS_QUERY_PATH: &str = "/dns-query";

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
	pub(crate) fn label(self) -> &'static str {
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
pub(crate) enum ServerHost {
	Ip(IpAddr),
	Name(String),
}

/// One entry of `dns.servers`, parsed at agent construction. The IP is not known yet for a
/// hostname host: that is resolved when the resolver is first used (see [`ResolverSettings`](crate::ResolverSettings)).
#[derive(Clone, Debug)]
pub struct ServerSpec {
	pub(crate) host: ServerHost,
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
	pub(crate) fn ip(&self) -> Option<IpAddr> {
		match self.host {
			ServerHost::Ip(ip) => Some(ip),
			ServerHost::Name(_) => None,
		}
	}

	/// The certificate name to authenticate against once the host resolves to `ip`: an explicit
	/// fragment, else the hostname, else the address itself.
	// spec:DNS#transports
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
	pub(crate) fn to_name_server(&self, ip: IpAddr) -> NameServerConfig {
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

#[cfg(test)]
mod tests;
