//! An HTTP/3 upgrade (via Alt-Svc primarily) mechanism for reqwest.
//!
//! An origin advertises HTTP/3 in an `Alt-Svc` header or an `HTTPS` DNS record. The alternative
//! service may be unreachable even when advertised, and trying would unnecessarily fail and waste
//! a request.
//!
//! [`AltSvcCache`] is an in-memory store which keeps track of these advertisements, and decides
//! per origin whether HTTP/3 is worth attempting.
//!
//! [`AltSvcMiddleware`] then acts on that decision. There are two flavours, depending on whether
//! you want to do background probes (with [`H3Prober`]) or not:
//!
//! - With, advertisements are verified in the background, and foreground requests use HTTP/3 only
//!   once an origin is confirmed, so no user-visible request pays for discovering a broken
//!   alternative service.
//! - Without, the next foreground request is itself the verification, falling back to TCP if it
//!   does not produce headers in time.
//!
//! The middleware also monitors connections and intelligently demotes and promotes origins between
//! QUIC and TCP:
//!
//! - if QUIC connections to the origin start failing,
//! - if the QUIC path becomes noticeably slower than the TCP path ([`PathTime`]),
//! - retries the upgrade after an exponential cooldown.
//!
//! # Examples
//!
//! ```
//! use reqwest::Url;
//! use web_faith_alt_svc::{AltSvcAdvertisement, AltSvcCache, AltSvcCacheConfig};
//!
//! let cache = AltSvcCache::new(AltSvcCacheConfig::default());
//! let origin = Url::parse("https://example.com/").expect("a valid URL");
//!
//! // Inject an advertisement; usually this would either be a provided hint or come from the
//! // middleware.
//! let advertised: AltSvcAdvertisement = r#"h3=":443"; ma=86400"#.parse().expect("h3 is advertised");
//! cache.record_alt_svc(&origin, &advertised);
//! assert_eq!(cache.confirmed_port(&origin), None);
//!
//! // Now that there's an advertisement, a probe against the origin has something to try.
//! let port = cache.probe_candidate(&origin).expect("worth probing");
//! cache.confirm_h3(&origin, port);
//! assert_eq!(cache.confirmed_port(&origin), Some(443));
//! ```
//!
#![cfg_attr(
	feature = "dns",
	doc = "Use [`H3HttpsSink`] to additionally support the DNS discovery of HTTP/3 services (using"
)]
#![cfg_attr(feature = "dns", doc = "the `HTTPS` record type).")]
#![cfg_attr(feature = "dns", doc = "")]
#![cfg_attr(feature = "dns", doc = "```no_run")]
#![cfg_attr(feature = "dns", doc = "use std::sync::Arc;")]
#![cfg_attr(feature = "dns", doc = "")]
#![cfg_attr(
	feature = "dns",
	doc = "use web_faith_alt_svc::{AltSvcCache, AltSvcCacheConfig, H3HttpsSink};"
)]
#![cfg_attr(
	feature = "dns",
	doc = "use web_faith_dns::{FaithResolver, ResolverSettings};"
)]
#![cfg_attr(feature = "dns", doc = "")]
#![cfg_attr(
	feature = "dns",
	doc = "let cache = Arc::new(AltSvcCache::new(AltSvcCacheConfig::default()));"
)]
#![cfg_attr(
	feature = "dns",
	doc = "let resolver = FaithResolver::new(ResolverSettings::default());"
)]
#![cfg_attr(feature = "dns", doc = "")]
#![cfg_attr(
	feature = "dns",
	doc = "// The resolver will now query for `HTTPS` records, and if any exist, populate the"
)]
#![cfg_attr(
	feature = "dns",
	doc = "// Alt-Svc cache even before we start connecting."
)]
#![cfg_attr(
	feature = "dns",
	doc = "resolver.set_https_sink(Arc::new(H3HttpsSink::new(cache, None)));"
)]
#![cfg_attr(feature = "dns", doc = "```")]
#![deny(missing_docs)]
// Lets docs.rs label each item with the feature or platform it needs.
#![cfg_attr(docsrs, feature(doc_cfg))]

// spec:H3UP spec:PROBE

mod cache;
mod header;

#[cfg(feature = "dns")]
mod https_sink;

mod middleware;
mod prober;

pub use cache::{AltSvcAdvertisement, AltSvcCache, AltSvcCacheConfig, AltSvcEntry, PathTime};
pub use header::NoHttp3Alternative;
#[cfg(feature = "dns")]
pub use https_sink::H3HttpsSink;
pub use middleware::AltSvcMiddleware;

pub use prober::H3Prober;
