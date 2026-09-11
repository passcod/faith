//! A browser-shaped HTTP client.
//!
//! Faith behaves like a browser ("faithfully") wherever that translates to a server-side runtime:
//! transparent HTTP/2 and HTTP/3 upgrades, Happy Eyeballs across IPv4 and IPv6, DNS caching, an
//! optional cookie jar, and HTTP caching. We also publish the reusable components as separate
//! crates.
//!
//! ```no_run
//! use web_faith::Agent;
//!
//! # async fn example() -> Result<(), web_faith::FaithError> {
//! let agent = Agent::new()?;
//! let body = agent.fetch("https://example.com/").await?.text().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # HTTP/3 is opt-in
//!
//! Faith uses reqwest internally, and its HTTP/3 support is currently unstable. To enable HTTP/3
//! support, you will need to set the `http3` feature on Faith, and use the `reqwest_unstable` rustc
//! cfg flag:
//!
//! ```toml
//! [dependencies]
//! web-faith = { version = "1.0", features = ["http3"] }
//! ```
//!
//! ```toml
//! # .cargo/config.toml
//! [build]
//! rustflags = ["--cfg", "reqwest_unstable"]
//! ```
//!
//! # Features
//!
//! | Feature | Default | What it adds |
//! | --- | --- | --- |
//! | `cache` | on | The HTTP cache, its store, and the per-request cache mode. |
//! | `connection-tracking` | on | Per-connection kernel counters, and the agent verb that reports them. |
//! | `cookies` | on | The cookie jar, and the agent option and handle that reach it. |
//! | `dns` | on | Faith's own caching resolver. Without it, names resolve through the platform. |
//! | `encoding` | on | Content codings: negotiating and decoding a response body, and compressing a request one. |
//! | `tls-aws-lc-rs` | on | aws-lc-rs as the rustls crypto provider. |
//! | `tls-ring` | off | ring as the rustls crypto provider instead. |
//! | `http3` | off | Transparent HTTP/3, and the Alt-Svc machinery that upgrades an origin to it. Needs the cfg flag above. |
//! | `raw-client` | off | `Agent::client` and `Agent::raw_client`, which hand out the reqwest client underneath. |
//! | `internals` | off | Faith's internals. Permanently unstable and exempt from semver. |
//!
//! # Component crates
//!
//! - [`web-faith-cookies`](https://docs.rs/web-faith-cookies)
//! - [`web-faith-dns`](https://docs.rs/web-faith-dns)
//! - [`web-faith-conn-tracker`](https://docs.rs/web-faith-conn-tracker)
//! - [`web-faith-alt-svc`](https://docs.rs/web-faith-alt-svc)
//! - [`web-faith-encoding`](https://docs.rs/web-faith-encoding)
//!
//! # Elsewhere
//!
//! Faith is also a Node.js module which lets you use this Rust networking stack as a `fetch`
//! drop-in replacement: [`@passcod/faith`](https://www.npmjs.com/package/@passcod/faith).

// A build with no crypto provider cannot speak TLS, and an HTTPS client that cannot is not one.
// Selecting a provider is therefore a choice between the two rather than an option to decline.
#[cfg(not(any(feature = "tls-aws-lc-rs", feature = "tls-ring")))]
compile_error!("web-faith needs a TLS backend: enable either tls-aws-lc-rs or tls-ring");

pub mod agent;
pub mod error;
pub mod request;
pub mod response;

mod builder;
mod client;
mod integrity;
mod retry;
mod stats;
mod timing;
mod warm_up;

// The types the ordinary builder path needs are re-exported from `agent` either way; `internals`
// adds the module path, for building `AgentOptions` field by field.
#[cfg(feature = "internals")]
pub mod body;
#[cfg(not(feature = "internals"))]
mod body;

#[cfg(feature = "internals")]
pub mod options;
#[cfg(not(feature = "internals"))]
mod options;

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

pub use agent::Agent;
pub use error::FaithError;
pub use request::Request;
pub use response::Response;

#[cfg(feature = "internals")]
pub use error::error_codes;
