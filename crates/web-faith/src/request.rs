//! Requests: preparing one, and the builders that send it.

// spec:REQ spec:ENC spec:CANCEL

mod builder;
mod parts;
mod send;
mod target;

pub use builder::{FetchBuilder, Priority, Request, RequestBuilder};
pub use target::Target;

// Compiled and used internally either way; `internals` decides whether they are nameable outside.
#[cfg(not(feature = "internals"))]
pub(crate) use parts::{RequestBody, RequestOptions};
#[cfg(feature = "internals")]
pub use {
	parts::{RequestBody, RequestOptions},
	send::send,
};

/// Whether a request carries its credentials.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Credentials {
	/// Strip credentials from the URL and send no cookies.
	Omit,
	/// Send them. Almost always what a server-side caller means.
	#[default]
	Include,
}

/// The methods the fetch standard normalises to upper case; any other method is sent as given.
// spec:REQ#method-and-headers
const NORMALISED_METHODS: [&str; 6] = ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"];

/// The method whose query content rides in the body, and so must be described by a `Content-Type`.
///
/// `http` has no constant for it: `QUERY` is still a draft method, so it arrives as an extension
/// method and is compared by name.
// spec:REQ#body
pub(crate) const QUERY: &str = "QUERY";

/// The header a request's priority is expressed in.
// spec:REQ#request-priority
pub(crate) const PRIORITY: &str = "priority";
