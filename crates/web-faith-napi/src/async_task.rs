use std::{fmt::Debug, future::Future, result::Result};

use napi::{
	Env,
	bindgen_prelude::*,
	sys::{napi_env, napi_value},
};
use serde_json;

use crate::error::FaithError;

#[derive(Clone, Debug)]
pub struct Value(pub serde_json::Value);

impl TypeName for Value {
	fn type_name() -> &'static str {
		"unknown"
	}

	fn value_type() -> ValueType {
		ValueType::Unknown
	}
}

impl ToNapiValue for Value {
	unsafe fn to_napi_value(env: napi_env, val: Self) -> Result<napi_value, napi::Error> {
		unsafe { serde_json::Value::to_napi_value(env, val.0) }
	}
}

/// Spawn a future on the shared tokio runtime and return a JS Promise for it.
///
/// The future runs entirely on the tokio runtime: unlike `AsyncTask`, no libuv
/// worker thread is occupied while the future is pending, so any number of
/// requests can be in flight concurrently regardless of `UV_THREADPOOL_SIZE`.
///
/// Errors are converted on the JS thread via [`FaithError::into_js_error`], so
/// rejections keep their proper JS error class (TypeError, SyntaxError, named
/// errors) and their `.code` property.
pub fn faith_promise<'env, T, F>(env: &'env Env, fut: F) -> Result<PromiseRaw<'env, T>, napi::Error>
where
	T: ToNapiValue + Send + 'static,
	F: Future<Output = Result<T, FaithError>> + Send + 'static,
{
	env.spawn_future_with_callback(
		async move { Ok(fut.await) },
		|env, result: Result<T, FaithError>| match result {
			Ok(value) => Ok(value),
			Err(error) => Err(napi::Error::from(error.into_js_error(env))),
		},
	)
}
