use std::{fmt::Debug, sync::Arc};

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::agent::FaithAgent;

#[napi(object)]
pub struct FaithOptionsAndBody {
    pub method: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub body: Option<Either3<String, Buffer, Uint8Array>>,
    pub timeout: Option<f64>,
    pub agent: Reference<FaithAgent>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FaithOptions {
    pub(crate) method: Option<String>,
    pub(crate) headers: Option<Vec<(String, String)>>,
    pub(crate) timeout: Option<f64>,
}

impl FaithOptions {
    pub(crate) fn extract(opts: FaithOptionsAndBody) -> (Self, FaithAgent, Option<Arc<Buffer>>) {
        (
            Self {
                method: opts.method,
                headers: opts.headers,
                timeout: opts.timeout,
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
