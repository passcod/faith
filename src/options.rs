use std::{fmt::Debug, sync::Arc, time::Duration};

use http_cache_reqwest::CacheMode;
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::agent::Agent;

/// The cache mode you want to use for the request. This may be any one of the following values:
///
/// - `default`: The client looks in its HTTP cache for a response matching the request.
///   - If there is a match and it is fresh, it will be returned from the cache.
///   - If there is a match but it is stale, the client will make a conditional request to the remote
///     server. If the server indicates that the resource has not changed, it will be returned from the
///     cache. Otherwise the resource will be downloaded from the server and the cache will be updated.
///   - If there is no match, the client will make a normal request, and will update the cache with
///     the downloaded resource.
///
/// - `no-store`: The client fetches the resource from the remote server without first looking in the
///   cache, and will not update the cache with the downloaded resource.
///
/// - `reload`: The client fetches the resource from the remote server without first looking in the
///   cache, but then will update the cache with the downloaded resource.
///
/// - `no-cache`: The client looks in its HTTP cache for a response matching the request.
///   - If there is a match, fresh or stale, the client will make a conditional request to the remote
///     server. If the server indicates that the resource has not changed, it will be returned from the
///     cache. Otherwise the resource will be downloaded from the server and the cache will be updated.
///   - If there is no match, the client will make a normal request, and will update the cache with
///     the downloaded resource.
///
/// - `force-cache`: The client looks in its HTTP cache for a response matching the request.
///   - If there is a match, fresh or stale, it will be returned from the cache.
///   - If there is no match, the client will make a normal request, and will update the cache with
///     the downloaded resource.
///
/// - `only-if-cached`: The client looks in its HTTP cache for a response matching the request.
///   - If there is a match, fresh or stale, it will be returned from the cache.
///   - If there is no match, a network error is returned.
///
/// - `ignore-rules`: Custom to Fáith. Overrides the check that determines if a response can be cached
///   to always return true on 200. Uses any response in the HTTP cache matching the request, not
///   paying attention to staleness. If there was no response, it creates a normal request and updates
///   the HTTP cache with the response.
#[napi(string_enum, js_name = "CacheMode")]
#[derive(Debug, Clone, Copy, Default)]
pub enum RequestCacheMode {
	#[napi(value = "default")]
	#[default]
	Default,

	#[napi(value = "force-cache")]
	ForceCache,

	#[napi(value = "ignore-rules")]
	IgnoreRules,

	#[napi(value = "no-cache")]
	NoCache,

	#[napi(value = "no-store")]
	NoStore,

	#[napi(value = "only-if-cached")]
	OnlyIfCached,

	#[napi(value = "reload")]
	Reload,
}

impl From<RequestCacheMode> for CacheMode {
	fn from(mode: RequestCacheMode) -> Self {
		match mode {
			RequestCacheMode::Default => Self::Default,
			RequestCacheMode::ForceCache => Self::ForceCache,
			RequestCacheMode::IgnoreRules => Self::IgnoreRules,
			RequestCacheMode::NoCache => Self::NoCache,
			RequestCacheMode::NoStore => Self::NoStore,
			RequestCacheMode::OnlyIfCached => Self::OnlyIfCached,
			RequestCacheMode::Reload => Self::Reload,
		}
	}
}

/// Controls whether or not the client sends credentials with the request, as well as whether any
/// `Set-Cookie` response headers are respected. Credentials are cookies, ~~TLS client certificates,~~
/// or authentication headers containing a username and password. This option may be any one of the
/// following values:
///
/// - `omit`: Never send credentials in the request or include credentials in the response.
/// - ~~`same-origin`~~: Fáith does not implement this, as there is no concept of "origin" on the server.
/// - `include`: Always include credentials, ~~even for cross-origin requests.~~
///
/// Fáith ignores the `Access-Control-Allow-Credentials` and `Access-Control-Allow-Origin` headers.
///
/// Fáith currently does not `omit` the TLS client certificate when the request's `Agent` has one
/// configured. This is an upstream limitation.
///
/// If the request's `Agent` has cookies enabled, new cookies from the response will be added to the
/// cookie jar, even as Fáith strips them from the request and response headers returned to the user.
/// This is an upstream limitation.
///
/// Defaults to `include` (browsers default to `same-origin`).
#[napi(string_enum)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialsOption {
	#[napi(value = "omit")]
	Omit,
	#[napi(value = "same-origin")]
	SameOrigin,
	#[napi(value = "include")]
	Include,
}

impl Default for CredentialsOption {
	fn default() -> Self {
		CredentialsOption::Include
	}
}

/// Controls duplex behavior of the request. If this is present it must have the value `half`, meaning
/// that Fáith will send the entire request before processing the response.
///
/// This option must be present when `body` is a `ReadableStream`.
#[napi(string_enum)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplexOption {
	#[napi(value = "half")]
	Half,
}

#[napi(object)]
pub struct FaithOptionsAndBody {
	pub agent: Reference<Agent>,
	pub body: Option<Either3<String, Buffer, Uint8Array>>,
	pub cache: Option<RequestCacheMode>,
	pub credentials: Option<CredentialsOption>,
	pub duplex: Option<DuplexOption>,
	pub headers: Option<Vec<(String, String)>>,
	pub integrity: Option<String>,
	pub method: Option<String>,
	pub timeout: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FaithOptions {
	pub(crate) cache: RequestCacheMode,
	pub(crate) credentials: CredentialsOption,
	pub(crate) headers: Option<Vec<(String, String)>>,
	pub(crate) integrity: Option<String>,
	pub(crate) method: Option<String>,
	pub(crate) timeout: Option<Duration>,
}

impl FaithOptions {
	pub(crate) fn extract(opts: FaithOptionsAndBody) -> (Self, Agent, Option<Arc<Buffer>>) {
		let credentials = opts.credentials.unwrap_or_default();
		// Transform same-origin to include
		let credentials = if credentials == CredentialsOption::SameOrigin {
			CredentialsOption::Include
		} else {
			credentials
		};

		(
			Self {
				cache: opts.cache.unwrap_or_default(),
				credentials,
				headers: opts.headers,
				integrity: opts.integrity,
				method: opts.method,
				timeout: opts.timeout.map(Into::into).map(Duration::from_millis),
			},
			Agent::clone(&opts.agent),
			opts.body.map(|either| match either {
				Either3::A(s) => Arc::new(Buffer::from(s.as_bytes())),
				Either3::B(b) => Arc::new(b),
				Either3::C(u) => Arc::new(Buffer::from(u.as_ref())),
			}),
		)
	}
}
