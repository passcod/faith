//! A caching DNS resolver for HTTP clients, with a cache you can warm.
//!
//! A client's built-in resolver usually keeps its cache to itself, so the only way to populate it is
//! to make a request. That is no good for prefetching a name ahead of time, which must not touch the
//! origin at all. [`FaithResolver`] is the resolver instead: it can be installed on an HTTP client
//! so every request resolves through it, while [`FaithResolver::prefetch`] is called directly. Both
//! share one resolver and one cache, so a name warmed ahead of time is already there when a request
//! looks it up.
//!
//! # Transports and server order
//!
//! Resolvers are named by URL, and the scheme picks the transport: plaintext `udp` and `tcp`, or
//! encrypted `tls`, `https`, `quic`, and `h3`. The list is queried in the order given rather than
//! reordered by latency. Given no list, the resolver configures itself from the operating system and
//! lets RFC 9539 opportunistic encryption upgrade those servers where it can.
//!
//! [Exempt names] are sent to the system resolver whichever way the rest is configured, so names
//! that only the host knows how to resolve keep resolving.
//!
//! # Beyond addresses
//!
//! Lookups can also read the `HTTPS` record for a name, which is how an origin advertises HTTP/3
//! before anything has connected to it, and answers can be served stale while a fresh lookup runs.
//! A [network change][FaithResolver::reset] discards what was learned from a network that no longer
//! exists while leaving the resolver usable.
//!
//! [Exempt names]: ResolverSettings::exempt_domains
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
