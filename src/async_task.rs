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

pub type Async<T> = AsyncTask<FaithAsyncResult<T>>;
pub struct FaithAsyncResult<T>(Pin<Box<dyn Future<Output = Result<T, FaithError>> + Send>>)
where
    T: Send + ToNapiValue + TypeName + 'static;

impl<T> FaithAsyncResult<T>
where
    T: Send + ToNapiValue + TypeName + 'static,
{
    pub fn run<F, U>(f: F) -> AsyncTask<Self>
    where
        F: Fn() -> U + Send + 'static,
        U: Future<Output = Result<T, FaithError>> + Send + 'static,
    {
        AsyncTask::new(Self(Box::pin(f())))
    }
}

impl<'env, T> ScopedTask<'env> for FaithAsyncResult<T>
where
    T: Send + ToNapiValue + TypeName + 'static,
{
    type Output = Result<T, FaithError>;
    type JsValue = T;

    fn compute(&mut self) -> Result<Self::Output, napi::Error> {
        match Handle::try_current() {
            Ok(handle) => Ok(handle.block_on(&mut self.0)),
            Err(err) if err.is_missing_context() => {
                let rt = Runtime::new().map_err(|err| {
                    FaithError::new(FaithErrorKind::RuntimeThread, Some(err.to_string()))
                        .into_napi()
                })?;
                Ok(rt.block_on(&mut self.0))
            }
            Err(err) => Err(
                FaithError::new(FaithErrorKind::RuntimeThread, Some(err.to_string())).into_napi(),
            ),
        }
    }

    fn resolve(
        &mut self,
        env: &'env Env,
        output: Self::Output,
    ) -> Result<Self::JsValue, napi::Error> {
        match output {
            Ok(t) => Ok(t),
            Err(err) => Err(napi::Error::from(err.into_js_error(env))),
        }
    }

    fn reject(&mut self, _env: &'env Env, err: Error) -> Result<Self::JsValue, napi::Error> {
        // TODO: we could probably add .code to the error here, by converting the napi::Error
        // back into a FaithError, then to_js_error(), then From<Unknown> for napi::Error
        Err(err)
    }

    fn finally(self, _: Env) -> Result<(), napi::Error> {
        drop(self.0);
        Ok(())
    }
}
