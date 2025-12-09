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
    pub agent: Option<Reference<FaithAgent>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FaithOptions {
    pub(crate) method: Option<String>,
    pub(crate) headers: Option<Vec<(String, String)>>,
    pub(crate) timeout: Option<f64>,
}

impl FaithOptions {
    pub(crate) fn extract(
        opts: Option<FaithOptionsAndBody>,
    ) -> (Self, Option<FaithAgent>, Option<Arc<Buffer>>) {
        match opts {
            None => (Self::default(), None, None),
            Some(opts) => (
                Self {
                    method: opts.method,
                    headers: opts.headers,
                    timeout: opts.timeout,
                },
                opts.agent.map(|rf| FaithAgent::clone(&rf)),
                opts.body.map(|either| match either {
                    Either3::A(s) => Arc::new(Buffer::from(s.as_bytes())),
                    Either3::B(b) => Arc::new(b),
                    Either3::C(u) => Arc::new(Buffer::from(u.as_ref())),
                }),
            ),
        }
    }
}
