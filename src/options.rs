use std::{fmt::Debug, sync::Arc, time::Duration};

use http_cache_reqwest::CacheMode;
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::agent::Agent;

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
	pub method: Option<String>,
	pub timeout: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FaithOptions {
	pub(crate) cache: RequestCacheMode,
	pub(crate) credentials: CredentialsOption,
	pub(crate) headers: Option<Vec<(String, String)>>,
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
