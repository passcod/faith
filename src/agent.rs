use napi::Env;
use napi_derive::napi;
use reqwest::Client;

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
#[derive(Debug, Clone, Default)]
pub struct AgentOptions {
    pub user_agent: Option<String>,
}

#[napi]
#[derive(Debug, Clone)]
pub struct Agent {
    pub(crate) client: Client,
}

#[napi]
impl Agent {
    pub fn new() -> Result<Self, FaithError> {
        Self::with_options(AgentOptions::default())
    }

    pub fn with_options(options: AgentOptions) -> Result<Self, FaithError> {
        let client = Client::builder()
            .user_agent(options.user_agent.as_deref().unwrap_or(USER_AGENT))
            .build()?;

        Ok(Self { client })
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
}
