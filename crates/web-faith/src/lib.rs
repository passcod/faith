//! A browser-shaped HTTP client: fetch semantics over a Rust network stack.
//!
//! Faith behaves like a browser wherever that translates to a server-side runtime: transparent
//! HTTP/2 and HTTP/3, Happy Eyeballs across IPv4 and IPv6, DNS caching, an optional cookie jar, and
//! HTTP caching. The subsystems beneath it are published on their own, and each can be left out of a
//! build with the feature named for it.
//!
//! Whichever layer a request fails in, the failure arrives as one [`FaithError`] whose
//! [`FaithErrorKind`] is the stable code to match on: a component crate names its own errors, and
//! they are converted at the boundary as they cross into the client.
//!
//! <div class="warning">
//!
//! The caller-facing API is still being shaped. Everything the client does is here -- building an
//! agent, sending a request, reading a response -- but it is reached through [`request::send`] and
//! the modules below rather than through the fetch-flavoured builder that will front it.
//!
//! </div>

pub mod agent;
pub mod body;
pub mod client;
pub mod error;
pub mod integrity;
pub mod request;
pub mod response;
pub mod retry;
pub mod stats;
pub mod timing;
pub mod warm_up;

/// The `User-Agent` a request carries when nothing overrides it.
///
/// Prepend your own product token to it rather than replacing it, so a server still sees which
/// client is calling:
///
/// ```
/// # use web_faith::USER_AGENT;
/// let ua = format!("YourApp/1.2.3 {USER_AGENT}");
/// assert!(ua.ends_with(USER_AGENT));
/// ```
pub const USER_AGENT: &str = concat!(
	"Faith/",
	env!("CARGO_PKG_VERSION"),
	" reqwest/",
	env!("REQWEST_VERSION")
);

pub use error::{FaithError, FaithErrorKind, error_codes};
