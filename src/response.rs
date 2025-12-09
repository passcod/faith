use std::{
    fmt::Debug,
    result::Result,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use bytes::Bytes;
use futures::{StreamExt, TryStreamExt};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde_json;

use crate::{
    async_task::{Async, FaithAsyncResult, Value},
    body::Body,
    error::{FaithError, FaithErrorKind},
};

#[napi]
#[derive(Debug)]
pub struct FaithResponse {
    pub(crate) status: u16,
    pub(crate) status_text: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) ok: bool,
    pub(crate) url: String,
    pub(crate) redirected: bool,
    pub(crate) disturbed: Arc<AtomicBool>,
    pub(crate) inner_body: Body,
}

impl Clone for FaithResponse {
    fn clone(&self) -> Self {
        Self {
            status: self.status,
            status_text: self.status_text.clone(),
            headers: self.headers.clone(),
            ok: self.ok,
            url: self.url.clone(),
            redirected: self.redirected,
            disturbed: Arc::clone(&self.disturbed),
            inner_body: self.inner_body.clone(),
        }
    }
}

#[napi]
impl FaithResponse {
    #[napi(getter)]
    pub fn status(&self) -> u16 {
        self.status
    }

    #[napi(getter)]
    pub fn status_text(&self) -> String {
        self.status_text.clone()
    }

    #[napi(getter)]
    pub fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    #[napi(getter)]
    pub fn ok(&self) -> bool {
        self.ok
    }

    #[napi(getter)]
    pub fn url(&self) -> String {
        self.url.clone()
    }

    #[napi(getter)]
    pub fn redirected(&self) -> bool {
        self.redirected
    }

    /// Check if the response body has been disturbed (read)
    #[napi(getter)]
    pub fn body_used(&self) -> bool {
        self.disturbed.load(Ordering::SeqCst)
    }

    /// Get the response body as a ReadableStream
    #[napi]
    pub fn body(
        &self,
        env: Env,
    ) -> Result<Option<napi::bindgen_prelude::ReadableStream<'_, BufferSlice<'_>>>, napi::Error>
    {
        // we mark the body as disturbed, but we still allow reading it through here
        // as essentially, the body() can be accessed many times as the same stream
        let _ = self.check_stream_disturbed();

        match &self.inner_body {
            Body::None => Ok(None),
            Body::Stream(stream) => {
                let stream = stream.clone();
                let stream = napi::bindgen_prelude::ReadableStream::create_with_stream_bytes(
                    &env,
                    stream.map_err(|err| {
                        FaithError::new(FaithErrorKind::BodyStream, Some(err)).into_napi()
                    }),
                )?;
                Ok(Some(stream))
            }
        }
    }

    fn check_stream_disturbed(&self) -> Result<(), FaithError> {
        if self.disturbed.swap(true, Ordering::SeqCst) {
            Err(FaithErrorKind::ResponseAlreadyDisturbed.into())
        } else {
            Ok(())
        }
    }

    /// Underlying efficient response body fetcher.
    ///
    /// Unlike bytes() and co, this grabs all the chunks of the response but doesn't
    /// copy them. Further processing is needed to obtain a Vec<u8> or whatever needed.
    async fn gather(&self) -> Result<Arc<[Bytes]>, FaithError> {
        // Clone the stream before reading so we don't need &mut self
        let mut response = match &self.inner_body {
            Body::None => return Ok(Default::default()),
            Body::Stream(body) => body.clone(),
        };

        let mut chunks = Vec::new();
        while let Some(chunk) = response
            .next()
            .await
            .transpose()
            .map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)))?
        {
            chunks.push(chunk);
        }

        Ok(Arc::from(chunks.into_boxed_slice()))
    }

    /// gather() and then copy into one contiguous buffer
    async fn gather_contiguous(&self) -> Result<Buffer, FaithError> {
        let body = self.gather().await?;
        let length = body.iter().map(|chunk| chunk.len()).sum();
        let mut bytes = Vec::with_capacity(length);
        for chunk in body.into_iter() {
            bytes.extend_from_slice(chunk);
        }
        Ok(bytes.into())
    }

    /// Get response body as bytes
    ///
    /// This may use up to 2x the amount of memory that the response body takes
    /// when the Response is cloned() and will create a full copy of the data.
    #[napi]
    pub fn bytes(&self) -> Async<Buffer> {
        let this = Clone::clone(&*self);
        FaithAsyncResult::run(move || {
            let this = Clone::clone(&this);
            async move {
                this.check_stream_disturbed()?;
                let buf = this.gather_contiguous().await?;
                Ok(buf)
            }
        })
    }

    /// Convert response body to text (UTF-8)
    #[napi]
    pub fn text(&self) -> Async<String> {
        let this = Clone::clone(&*self);
        FaithAsyncResult::run(move || {
            let this = Clone::clone(&this);
            async move {
                this.check_stream_disturbed()?;
                let bytes = this.gather_contiguous().await?;
                String::from_utf8(bytes.to_vec()).map_err(|e| {
                    FaithError::new(FaithErrorKind::Generic, Some(e.to_string())).into()
                })
            }
        })
    }

    /// Parse response body as JSON
    #[napi]
    pub fn json(&self) -> Async<Value> {
        let this = Clone::clone(&*self);
        FaithAsyncResult::run(move || {
            let this = Clone::clone(&this);
            async move {
                this.check_stream_disturbed()?;
                let bytes = this.gather_contiguous().await?;
                let value = serde_json::from_slice(&bytes)
                    .map_err(|e| FaithError::new(FaithErrorKind::JsonParse, Some(e.to_string())))?;
                Ok(Value(value))
            }
        })
    }

    /// Get response body as ArrayBuffer (returns Buffer, wrapper converts to ArrayBuffer)
    #[napi]
    pub fn array_buffer(&self) -> Async<Buffer> {
        self.bytes()
    }

    /// Create a clone of the response
    ///
    /// Specially, this doesn't set the disturbed flag, so that `body()` or other such
    /// methods can work afterwards. However, it will throw if the body has already
    /// been read from.
    ///
    /// Clones will cache in memory the section of the response body that is read
    /// from one clone and not yet consumed by all others. In the worst case, you can
    /// end up with a copy of the entire response body if you end up not consuming one
    /// of the clones.
    #[napi]
    pub fn clone(&self, env: Env) -> Result<Self, napi::Error> {
        if self.disturbed.load(Ordering::SeqCst) {
            return Err(FaithError::from(FaithErrorKind::ResponseAlreadyDisturbed)
                .into_js_error(&env)
                .into());
        }

        Ok(Self {
            status: self.status,
            status_text: self.status_text.clone(),
            headers: self.headers.clone(),
            ok: self.ok,
            url: self.url.clone(),
            redirected: self.redirected,
            disturbed: Arc::new(AtomicBool::new(false)),
            inner_body: self.inner_body.clone(),
        })
    }
}
