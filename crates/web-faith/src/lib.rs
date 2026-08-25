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
//! ```no_run
//! # use web_faith::agent::Agent;
//! # async fn example() -> Result<(), web_faith::FaithError> {
//! let agent = Agent::new()?;
//! let body = agent.fetch("https://example.com/").await?.text().await?;
//! # Ok(())
//! # }
//! ```
//!
//! An [`Agent`] owns the connection pool, resolver, cookie jar, and caches; [`Agent::builder`]
//! configures one. Cloning an agent is cheap and every clone names the same one.
//!
//! [`Agent::fetch`] returns a builder that sends when awaited, so there is no separate send step.
//! [`Request`] prepares one without sending it, to adjust at each call site or send unchanged on
//! more than one agent.
//!
//! [`Agent`]: agent::Agent
//! [`Agent::builder`]: agent::Agent::builder
//! [`Agent::fetch`]: agent::Agent::fetch
//! [`Request`]: request::Request

// A build with no crypto provider cannot speak TLS, and an HTTPS client that cannot is not one.
// Selecting a provider is therefore a choice between the two rather than an option to decline.
#[cfg(not(any(feature = "tls-aws-lc-rs", feature = "tls-ring")))]
compile_error!("web-faith needs a TLS backend: enable either tls-aws-lc-rs or tls-ring");

pub mod agent;
pub mod body;
pub mod builder;
pub mod client;
pub mod error;
pub mod integrity;
pub mod options;
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
