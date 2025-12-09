use std::{fmt::Debug, sync::Arc};

use napi::bindgen_prelude::*;
use napi_derive::napi;

#[napi(object)]
pub struct FaithOptionsAndBody {
    pub method: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub body: Option<Buffer>,
    pub timeout: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FaithOptions {
    pub(crate) method: Option<String>,
    pub(crate) headers: Option<Vec<(String, String)>>,
    pub(crate) timeout: Option<f64>,
}

impl FaithOptions {
    pub(crate) fn extract(opts: Option<FaithOptionsAndBody>) -> (Self, Option<Arc<Buffer>>) {
        match opts {
            None => (Self::default(), None),
            Some(opts) => (
                Self {
                    method: opts.method,
                    headers: opts.headers,
                    timeout: opts.timeout,
                },
                opts.body.map(Arc::new),
            ),
        }
    }
}
