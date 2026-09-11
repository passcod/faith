//! A caching DNS resolver for HTTP clients.
//!
//! # Transports and server order
//!
//! The resolver is configured to consult a list of nameservers in order, or defaults to the system
//! configuration. Nameservers are specified by URL:
//!
//! - `udp://IP:PORT` uses classic plain text DNS over UDP, port 53 by default.
//! - `tcp://IP:PORT` the same over TCP, port 53 by default.
//! - `tls://IP:PORT` uses DNS over TLS ([RFC 7858](https://www.rfc-editor.org/rfc/rfc7858)), port
//!   853 by default.
//! - `https://HOST:PORT/PATH` uses DNS over HTTPS
//!   ([RFC 8484](https://www.rfc-editor.org/rfc/rfc8484)), port 443 and `/dns-query` by default.
//! - `quic://IP:PORT` uses DNS over QUIC ([RFC 9250](https://www.rfc-editor.org/rfc/rfc9250)),
//!   port 853 by default.
//! - `h3://HOST:PORT/PATH` uses DNS over HTTP/3, port 443 and `/dns-query` by default.
//!
//! The encrypted transports always authenticate the nameserver. A hostname authenticates against
//! itself, a bare IP against the address, and a URL fragment (`tls://1.1.1.1#cloudflare-dns.com`)
//! gives the certificate to expect instead.
//!
//! When available, opportunistic encryption upgrade
//! ([RFC 9539](https://www.rfc-editor.org/rfc/rfc9539)) is used to secure nameservers.
//!
//! Some names are exempt from DNS resolution, and are always served by the system:
//!
//! - `localhost` and anything under it.
//! - `.local` and anything under it.
//! - The system's own DNS domain and search suffixes.
//! - Anything listed in [`exempt_domains`](ResolverSettings::exempt_domains).
//!
//! # Beyond addresses
//!
//! - Lookups can also read a name's `HTTPS` record, which is how an origin advertises HTTP/3
//!   before anything has connected to it.
//! - An answer can be served stale while a fresh lookup runs behind it.
//! - A [network change][FaithResolver::reset] discards what was learned from a network that no
//!   longer exists, leaving the resolver usable.
//!
//! ```no_run
//! use web_faith_dns::{FaithResolver, ResolverSettings};
//!
//! # async fn example() {
//! // No servers named, so it configures itself from the operating system.
//! let resolver = FaithResolver::new(ResolverSettings::default());
//!
//! // Warming is advisory and never fails; a later lookup reads the same cache.
//! resolver.prefetch("example.com").await;
//!
//! for report in resolver.resolvers() {
//!     println!("{} over {} ({})", report.address, report.transport, report.source);
//! }
//! # }
//! ```
//!
//! # Features
//!
//! The `reqwest` feature enables support to use this resolver with `reqwest::ClientBuilder`.

#![deny(missing_docs)]
// Lets docs.rs label each item with the feature or platform it needs.
#![cfg_attr(docsrs, feature(doc_cfg))]

// spec:WARM spec:DNS

mod discovery;
mod https;
mod resolver;
mod settings;
mod transport;

pub use https::{HttpsAdvertisement, HttpsSink};
pub use resolver::FaithResolver;
pub use settings::{DEFAULT_MAX_STALE, ResolverReport, ResolverSettings, ResolverSource};
pub use transport::{ServerSpec, Transport};

use hickory_resolver::proto::rr::Name;

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
