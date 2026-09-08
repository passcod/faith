//! An Alt-Svc store, and the machinery that upgrades an origin to HTTP/3 on the strength of it.
//!
//! An origin advertises HTTP/3 in an `Alt-Svc` header, or in an `HTTPS` DNS record. Acting on that
//! is not as simple as believing it: the alternative may be unreachable even though it was
//! advertised, and finding out costs the request that tries. What this does is keep the
//! advertisements ([`AltSvcCache`]) and decide, per origin, whether HTTP/3 is worth attempting.
//!
//! [`AltSvcMiddleware`] is the layer that acts on the decision. Two shapes are available:
//!
//! - With an [`H3Prober`], an advertisement is verified in the background and foreground requests
//!   are routed over HTTP/3 only once an origin is confirmed, so no user-visible request pays for
//!   discovering a broken alternative.
//! - Without one, the next foreground request is itself the verification, falling back to TCP if the
//!   attempt does not produce headers in time.
//!
//! Either way an origin that starts failing, or that turns out to be slower over HTTP/3 than the
//! path it replaced ([`PathTime`]), is demoted, and the cooldown before it is tried again lengthens
//! with each consecutive failure.
//!
//! [`parse_alt_svc_header`] reads a header on its own if all you want is the advertisement, and
//! [`H3HttpsSink`] feeds the store from `HTTPS` record lookups.

// spec:H3UP spec:PROBE

mod cache;
mod header;

#[cfg(feature = "dns")]
mod https_sink;

mod middleware;
mod prober;

pub use cache::{
	AltSvcAdvertisement, AltSvcCache, AltSvcCacheConfig, AltSvcEntry, PathTime, SLOW_FLOOR_MS,
};
pub use header::parse_alt_svc_header;
#[cfg(feature = "dns")]
pub use https_sink::H3HttpsSink;
pub use middleware::{AltSvcMiddleware, ArrivalStamp};

pub use prober::H3Prober;
