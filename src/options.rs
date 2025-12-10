use std::{fmt::Debug, sync::Arc, time::Duration};

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::agent::FaithAgent;

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
    pub method: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub body: Option<Either3<String, Buffer, Uint8Array>>,
    pub timeout: Option<u32>,
    pub credentials: Option<CredentialsOption>,
    pub duplex: Option<DuplexOption>,
    pub agent: Reference<FaithAgent>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FaithOptions {
    pub(crate) method: Option<String>,
    pub(crate) headers: Option<Vec<(String, String)>>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) credentials: CredentialsOption,
}

impl FaithOptions {
    pub(crate) fn extract(opts: FaithOptionsAndBody) -> (Self, FaithAgent, Option<Arc<Buffer>>) {
        let credentials = opts.credentials.unwrap_or_default();
        // Transform same-origin to include
        let credentials = if credentials == CredentialsOption::SameOrigin {
            CredentialsOption::Include
        } else {
            credentials
        };

        (
            Self {
                method: opts.method,
                headers: opts.headers,
                timeout: opts.timeout.map(Into::into).map(Duration::from_millis),
                credentials,
            },
            FaithAgent::clone(&opts.agent),
            opts.body.map(|either| match either {
                Either3::A(s) => Arc::new(Buffer::from(s.as_bytes())),
                Either3::B(b) => Arc::new(b),
                Either3::C(u) => Arc::new(Buffer::from(u.as_ref())),
            }),
        )
    }
}
