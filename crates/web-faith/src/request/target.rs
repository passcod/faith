//! A request's target.

use bytes::Bytes;
use url::Url;

use crate::{
	error::{FaithError, FaithErrorKind},
	request::{Request, RequestBody, RequestOptions},
};

/// A request's target: a URL, or another request to layer over.
///
/// Anything that converts into a `url::Url` is a target, as is a [`Request`], which is what lets a
/// prepared request be adjusted at each call site.
// spec:REQ
pub enum Target {
	/// A URL.
	Url(Url),
	/// A prepared request, to layer over.
	Request(Box<Request>),
}

impl From<Url> for Target {
	fn from(url: Url) -> Self {
		Self::Url(url)
	}
}

impl From<Request> for Target {
	fn from(request: Request) -> Self {
		Self::Request(Box::new(request))
	}
}

/// A string target is parsed when the builder resolves, so an unparseable one is reported there
/// rather than by a call that cannot fail.
// spec:REQ
impl TryFrom<&str> for Target {
	type Error = FaithError;

	fn try_from(url: &str) -> Result<Self, Self::Error> {
		Url::parse(url)
			.map(Self::Url)
			.map_err(|_| FaithErrorKind::InvalidUrl.into())
	}
}

impl TryFrom<String> for Target {
	type Error = FaithError;

	fn try_from(url: String) -> Result<Self, Self::Error> {
		Self::try_from(url.as_str())
	}
}

/// An `http::Request` is a target too, so a request built against the wider ecosystem can be sent
/// on an agent: its method, URL, headers, and body come across.
// spec:REQ
impl<B> TryFrom<http::Request<B>> for Target
where
	B: Into<Bytes>,
{
	type Error = FaithError;

	fn try_from(request: http::Request<B>) -> Result<Self, Self::Error> {
		let (parts, body) = request.into_parts();

		let url = Url::parse(&parts.uri.to_string())
			.map_err(|_| FaithError::from(FaithErrorKind::InvalidUrl))?;

		let mut headers = Vec::new();
		for (name, value) in parts.headers.iter() {
			let Ok(value) = value.to_str() else {
				return Err(FaithErrorKind::InvalidHeader.into());
			};
			headers.push((name.to_string(), value.to_owned()));
		}

		let body = body.into();
		Ok(Self::Request(Box::new(Request {
			url,
			options: RequestOptions {
				method: Some(parts.method.to_string()),
				headers: (!headers.is_empty()).then_some(headers),
				..RequestOptions::default()
			},
			body: if body.is_empty() {
				RequestBody::None
			} else {
				RequestBody::Bytes(body)
			},
		})))
	}
}
