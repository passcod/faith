//! A browser-shaped HTTP client: fetch semantics over a Rust network stack.
//!
//! The client is being assembled here; for now this crate carries the error type the whole family
//! reports through. A component crate names its own errors for the failures it can produce, and
//! they are converted into [`FaithError`] as they cross into the client, so a caller matches on one
//! type whichever layer failed.

pub mod error;

pub use error::{FaithError, FaithErrorKind, error_codes};
