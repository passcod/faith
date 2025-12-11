use std::{
	str::FromStr as _,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
};

use napi::Env;
use napi_derive::napi;
use reqwest::{
	Client, Url,
	cookie::{CookieStore, Jar},
	header::{HeaderMap, HeaderName, HeaderValue},
};

use crate::error::FaithError;

#[napi]
pub const FAITH_VERSION: &str = env!("CARGO_PKG_VERSION");
#[napi]
pub const REQWEST_VERSION: &str = env!("REQWEST_VERSION");
#[napi]
pub const USER_AGENT: &str = concat!(
	"Faith/",
	env!("CARGO_PKG_VERSION"),
	" reqwest/",
	env!("REQWEST_VERSION")
);

#[napi(object)]
#[derive(Debug, Clone)]
pub struct Header {
	pub name: String,
	pub value: String,
	pub sensitive: Option<bool>,
}

#[napi(string_enum)]
#[derive(Debug, Clone, Copy, Default)]
pub enum Http3Congestion {
	#[napi(value = "cubic")]
	#[default]
	Cubic,

	#[napi(value = "bbr1")]
	Bbr1,
}

#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentHttp3Options {
	pub congestion: Option<Http3Congestion>,
	pub max_idle_timeout: Option<u8>,
}

#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct AgentOptions {
	pub cookies: Option<bool>,
	pub headers: Option<Vec<Header>>,
	pub http3: Option<AgentHttp3Options>,
	pub user_agent: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct InnerAgentStats {
	pub requests_sent: AtomicU64,
	pub responses_received: AtomicU64,
}

#[napi]
#[derive(Debug, Clone, Default)]
pub struct AgentStats {
	pub requests_sent: i64,
	pub responses_received: i64,
}

#[napi]
#[derive(Debug, Clone)]
pub struct Agent {
	pub(crate) client: Client,
	pub(crate) cookie_jar: Option<Arc<Jar>>,
	pub(crate) stats: Arc<InnerAgentStats>,
}

#[napi]
impl Agent {
	pub fn new() -> Result<Self, FaithError> {
		Self::with_options(AgentOptions::default())
	}

	pub fn with_options(options: AgentOptions) -> Result<Self, FaithError> {
		let mut client = Client::builder()
			.tls_info(true)
			.user_agent(options.user_agent.as_deref().unwrap_or(USER_AGENT));

		let cookie_jar = if options.cookies.unwrap_or(false) {
			let jar = Arc::new(Jar::default());
			client = client.cookie_provider(jar.clone());
			Some(jar)
		} else {
			None
		};

		if let Some(headers) = options.headers
			&& !headers.is_empty()
		{
			let map = HeaderMap::from_iter(headers.into_iter().filter_map(
				|Header {
				     name,
				     value,
				     sensitive,
				 }| {
					let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
						return None;
					};

					let Ok(mut value) = HeaderValue::from_bytes(value.as_bytes()) else {
						return None;
					};

					if sensitive.unwrap_or(false) {
						value.set_sensitive(true);
					}

					Some((name, value))
				},
			));
			client = client.default_headers(map);
		}

		#[cfg(feature = "http3")]
		if let Some(http3) = options.http3 {
			if let Some(Http3Congestion::Bbr1) = http3.congestion {
				client = client.http3_congestion_bbr();
			}

			if let Some(seconds) = http3.max_idle_timeout {
				use std::time::Duration;
				client = client
					.http3_max_idle_timeout(Duration::from_secs(seconds.min(120).max(1).into()));
			}
		}

		Ok(Self {
			client: client.build()?,
			cookie_jar,
			stats: Default::default(),
		})
	}

	#[napi(constructor)]
	pub fn construct(env: Env, options: Option<AgentOptions>) -> Result<Self, napi::Error> {
		Ok(if let Some(options) = options {
			Self::with_options(options)
		} else {
			Self::new()
		}
		.map_err(|err| err.into_js_error(&env))?)
	}

	#[napi]
	pub fn add_cookie(&self, url: String, cookie: String) {
		let Some(jar) = &self.cookie_jar else {
			return;
		};

		let Ok(url) = Url::from_str(&url) else {
			return;
		};

		jar.add_cookie_str(&cookie, &url);
	}

	#[napi]
	pub fn get_cookie(&self, url: String) -> Option<String> {
		let Some(jar) = &self.cookie_jar else {
			return None;
		};

		let Ok(url) = Url::from_str(&url) else {
			return None;
		};

		jar.cookies(&url)
			.and_then(|val| val.to_str().ok().map(ToOwned::to_owned))
	}

	#[napi]
	pub fn stats(&self) -> AgentStats {
		AgentStats {
			requests_sent: self
				.stats
				.requests_sent
				.load(Ordering::Relaxed)
				.try_into()
				.unwrap_or(i64::MAX),
			responses_received: self
				.stats
				.responses_received
				.load(Ordering::Relaxed)
				.try_into()
				.unwrap_or(i64::MAX),
		}
	}
}
