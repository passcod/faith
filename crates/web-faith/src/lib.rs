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
//! The client API is still being built out. What is here so far is the error type, the body and
//! timing machinery a response is built on, the retry layers in the request path, and the recipe
//! that builds the HTTP client itself.
//!
//! </div>

pub mod body;
pub mod client;
pub mod error;
pub mod retry;
pub mod stats;
pub mod timing;
pub mod warm_up;

pub use error::{FaithError, FaithErrorKind, error_codes};
