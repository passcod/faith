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
//! - Anything listed in [`exempt_domains`](ResolverConfig::exempt_domains).
//!
//! # Examples
//!
//! Consulting a named list of nameservers, in order:
//!
//! ```no_run
//! use web_faith_dns::{FaithResolver, ResolverConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let resolver = FaithResolver::new(ResolverConfig {
//!     servers: vec![
//!         "tls://1.1.1.1#cloudflare-dns.com".parse()?,
//!         "udp://9.9.9.9".parse()?,
//!     ],
//!     ..Default::default()
//! });
//!
//! resolver.prefetch("example.com").await;
//! # Ok(())
//! # }
//! ```
//!
//! Or from the system's own configuration:
//!
//! ```no_run
//! use web_faith_dns::{FaithResolver, ResolverConfig};
//!
//! # async fn example() {
//! let resolver = FaithResolver::new(ResolverConfig::default());
//!
//! resolver.prefetch("example.com").await;
//! for report in resolver.resolvers() {
//!     println!("{} over {} ({})", report.address, report.transport, report.source);
//! }
//! # }
//! ```
//!
//! # Warming the cache
//!
//! [`prefetch`](FaithResolver::prefetch) resolves a name ahead of the request that needs it.
//!
//! ```no_run
//! use web_faith_dns::{FaithResolver, ResolverConfig};
//!
//! # async fn example() {
//! let resolver = FaithResolver::new(ResolverConfig::default());
//! resolver.prefetch("example.com").await;
//! # }
//! ```
//!
//! # Beyond addresses
//!
//! Lookups can also read a queried name's `HTTPS` record. This is used to resolve an HTTP/3 server
//! address without first needing to connect to the HTTP/1 server.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use web_faith_dns::{FaithResolver, HttpsAdvertisement, HttpsSink, ResolverConfig};
//!
//! struct Upgrades;
//!
//! impl HttpsSink for Upgrades {
//!     fn wants(&self, _host: &str) -> bool {
//!         true
//!     }
//!
//!     fn record(&self, host: &str, advertisement: HttpsAdvertisement) {
//!         println!("{host} advertises HTTP/3 on port {:?}", advertisement.port);
//!     }
//! }
//!
//! # async fn example() {
//! let resolver = FaithResolver::new(ResolverConfig::default());
//! resolver.set_https_sink(Arc::new(Upgrades));
//!
//! // Any lookup from here also asks for the `HTTPS` record.
//! resolver.prefetch("example.com").await;
//! # }
//! ```
//!
//! To avoid delays and momentary outages, the resolver will answer a query with a stale entry from
//! cache while looking up the updated answer in the background for future queries.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use web_faith_dns::{FaithResolver, ResolverConfig};
//!
//! # async fn example() {
//! let resolver = FaithResolver::new(ResolverConfig {
//!     serve_stale: Some(Duration::from_secs(3600)),
//!     ..Default::default()
//! });
//!
//! resolver.prefetch("example.com").await;
//! // Whether the next lookup would be answered from an expired entry.
//! println!("serving stale: {}", resolver.served_stale("example.com"));
//! # }
//! ```
//!
//! The resolver can be instructed to clear its caches and other learned information at runtime,
//! for example to handle network-change events.
//!
//! ```no_run
//! use web_faith_dns::{FaithResolver, ResolverConfig};
//!
//! # async fn example() {
//! let resolver = FaithResolver::new(ResolverConfig::default());
//! resolver.prefetch("example.com").await;
//!
//! // The interface changed, so what was learned about the old network goes.
//! resolver.reset();
//! # }
//! ```
//!
//! Domain lists are built from [`Name`], re-exported here so a caller needs no hickory dependency:
//!
//! ```
//! use web_faith_dns::{Name, ResolverConfig};
//!
//! let config = ResolverConfig {
//!     exempt_domains: vec![Name::from_utf8("corp.internal").expect("a valid domain")],
//!     ..Default::default()
//! };
//! assert_eq!(config.exempt_domains.len(), 1);
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

pub use hickory_resolver::proto::rr::Name;

pub use https::{HttpsAdvertisement, HttpsSink};
pub use resolver::FaithResolver;
pub use settings::{DEFAULT_MAX_STALE, ResolverConfig, ResolverReport, ResolverSource};
pub use transport::{ServerSpec, ServerSpecError, Transport};

/// Parse a list of domain names, for the search or exempt lists, or return a message
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
