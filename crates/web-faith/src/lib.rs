//! A browser-shaped HTTP client: fetch semantics over a Rust network stack.
//!
//! The client is being assembled here. What it already owns is the error type the whole family
//! reports through, the body and timing machinery a response is built on, and the retry layers that
//! sit in the request path. A component crate names its own errors for the failures it can produce,
//! and they are converted into [`FaithError`] as they cross into the client, so a caller matches on
//! one type whichever layer failed.

pub mod body;
pub mod error;
pub mod retry;
pub mod timing;

pub use error::{FaithError, FaithErrorKind, error_codes};
