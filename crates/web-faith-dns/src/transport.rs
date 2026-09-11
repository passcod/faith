//! Resolver transports.
use std::{fmt, net::IpAddr, str::FromStr, sync::Arc};

use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ProtocolConfig};
use url::{Host, ParseError, Url};

/// The default DoH/DoQ query path, used when a `https://`/`h3://` server URL supplies none.
const DEFAULT_DNS_QUERY_PATH: &str = "/dns-query";

/// A transport to reach a nameserver over, chosen by a server URL's scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Transport {
	/// Plaintext DNS over UDP, port 53. `udp://`.
	Udp,
	/// Plaintext DNS over TCP, port 53. `tcp://`.
	Tcp,
	/// DNS over TLS, port 853. `tls://`.
	Tls,
	/// DNS over HTTPS, port 443. `https://`.
	Https,
	/// DNS over QUIC, port 853. `quic://`.
	Quic,
	/// DNS over HTTP/3, port 443. `h3://`.
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

	/// The conventional port for the transport, used when the URL gives none.
	fn default_port(self) -> u16 {
		match self {
			Self::Udp | Self::Tcp => 53,
			Self::Tls | Self::Quic => 853,
			Self::Https | Self::H3 => 443,
		}
	}

	/// The lowercase label reported by `resolvers()`.
	/// The URL scheme this transport is named by.
	pub fn scheme(self) -> &'static str {
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

impl fmt::Display for Transport {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.scheme())
	}
}

/// Why a nameserver URL is not a [`ServerSpec`].
///
/// The [`Url`](Self::Url) variant carries [`url::ParseError`], so `url` is a public dependency of
/// this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerSpecError {
	/// The input is not a URL.
	Url(ParseError),
	/// The URL's scheme names no DNS transport.
	UnknownScheme,
	/// The URL carries no host to send queries to.
	NoHost,
}

impl fmt::Display for ServerSpecError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Url(err) => write!(f, "not a URL: {err}"),
			Self::UnknownScheme => f.write_str("unknown DNS transport scheme"),
			Self::NoHost => f.write_str("no host to query"),
		}
	}
}

impl std::error::Error for ServerSpecError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			Self::Url(err) => Some(err),
			_ => None,
		}
	}
}

impl From<ParseError> for ServerSpecError {
	fn from(err: ParseError) -> Self {
		Self::Url(err)
	}
}

/// A resolver Faith reaches by IP or by a hostname it bootstraps.
#[derive(Clone, Debug)]
pub(crate) enum ServerHost {
	Ip(IpAddr),
	Name(String),
}

/// One nameserver to query.
///
/// Parsed from a server URL with [`FromStr`]. A hostname host has no IP yet; that is resolved when
/// the resolver is first used.
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

impl FromStr for ServerSpec {
	type Err = ServerSpecError;

	fn from_str(input: &str) -> Result<Self, Self::Err> {
		let url = Url::parse(input)?;
		let transport =
			Transport::from_scheme(url.scheme()).ok_or(ServerSpecError::UnknownScheme)?;

		let host = match url.host() {
			Some(Host::Ipv4(ip)) => ServerHost::Ip(IpAddr::V4(ip)),
			Some(Host::Ipv6(ip)) => ServerHost::Ip(IpAddr::V6(ip)),
			Some(Host::Domain(name)) => ServerHost::Name(name.to_owned()),
			None => return Err(ServerSpecError::NoHost),
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
}

impl ServerSpec {
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
