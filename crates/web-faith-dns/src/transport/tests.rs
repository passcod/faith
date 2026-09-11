use std::{net::IpAddr, sync::Arc};

use hickory_resolver::config::ProtocolConfig;

use super::{DEFAULT_DNS_QUERY_PATH, ServerSpec, Transport};

fn spec(input: &str) -> ServerSpec {
	input.parse::<ServerSpec>().expect("valid server URL")
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
	assert!("ftp://1.1.1.1".parse::<ServerSpec>().is_err());
	assert!("not a url".parse::<ServerSpec>().is_err());
}
