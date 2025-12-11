use std::{fmt::Debug, pin::Pin, result::Result};

use napi::{
	ScopedTask,
	bindgen_prelude::*,
	sys::{napi_env, napi_value},
};
use serde_json;
use tokio::runtime::{Handle, Runtime};

use crate::error::{FaithError, FaithErrorKind};

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

pub type Async<A, T = A> = AsyncTask<FaithAsyncResult<T, A>>;
pub struct FaithAsyncResult<T, A = T>
where
	T: Send + ToNapiValue + TypeName + 'static,
	A: Send + ToNapiValue + TypeName + 'static,
{
	run: Pin<Box<dyn Future<Output = Result<A, FaithError>> + Send>>,
	finaliser: Box<dyn Fn(A, Env) -> T + Send>,
}

impl<T> FaithAsyncResult<T, T>
where
	T: Send + ToNapiValue + TypeName + 'static,
{
	pub fn run<F, U>(run: F) -> AsyncTask<Self>
	where
		F: FnOnce() -> U + Send + 'static,
		U: Future<Output = Result<T, FaithError>> + Send + 'static,
	{
		AsyncTask::new(Self {
			run: Box::pin(run()),
			finaliser: Box::new(|t, _| t),
		})
	}

	pub fn with_signal<F, U>(signal: Option<AbortSignal>, run: F) -> AsyncTask<Self>
	where
		F: FnOnce() -> U + Send + 'static,
		U: Future<Output = Result<T, FaithError>> + Send + 'static,
	{
		AsyncTask::with_optional_signal(
			Self {
				run: Box::pin(run()),
				finaliser: Box::new(|t, _| t),
			},
			signal,
		)
	}
}

impl<T, A> FaithAsyncResult<T, A>
where
	T: Send + ToNapiValue + TypeName + 'static,
	A: Send + ToNapiValue + TypeName + 'static,
{
	pub fn with_finaliser<F, U>(
		run: F,
		finaliser: impl (Fn(A, Env) -> T) + Send + 'static,
	) -> AsyncTask<Self>
	where
		F: FnOnce() -> U + Send + 'static,
		U: Future<Output = Result<A, FaithError>> + Send + 'static,
	{
		AsyncTask::new(Self {
			run: Box::pin(run()),
			finaliser: Box::new(finaliser),
		})
	}
}

impl<'env, T, A> ScopedTask<'env> for FaithAsyncResult<T, A>
where
	T: Send + ToNapiValue + TypeName + 'static,
	A: Send + ToNapiValue + TypeName + 'static,
{
	type Output = Result<A, FaithError>;
	type JsValue = T;

	fn compute(&mut self) -> Result<Self::Output, napi::Error> {
		within_runtime_if_available(|| match Handle::try_current() {
			Ok(handle) => Ok(handle.block_on(&mut self.run)),
			Err(err) if err.is_missing_context() => {
				let rt = Runtime::new().map_err(|err| {
					FaithError::new(FaithErrorKind::RuntimeThread, Some(err.to_string()))
						.into_napi()
				})?;
				Ok(rt.block_on(&mut self.run))
			}
			Err(err) => Err(
				FaithError::new(FaithErrorKind::RuntimeThread, Some(err.to_string())).into_napi(),
			),
		})
	}

	fn resolve(
		&mut self,
		env: &'env Env,
		output: Self::Output,
	) -> Result<Self::JsValue, napi::Error> {
		match output {
			Ok(t) => Ok((self.finaliser)(t, *env)),
			Err(err) => Err(napi::Error::from(err.into_js_error(env))),
		}
	}

	fn reject(&mut self, env: &'env Env, err: Error) -> Result<Self::JsValue, napi::Error> {
		// Wrap the napi::Error in a FaithError to add .code property
		let faith_error = FaithError::new(FaithErrorKind::RuntimeThread, Some(err.to_string()));
		Err(napi::Error::from(faith_error.into_js_error(env)))
	}

	fn finally(self, _: Env) -> Result<(), napi::Error> {
		drop(self.run);
		drop(self.finaliser);
		Ok(())
	}
}
