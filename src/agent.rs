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

#[napi]
#[derive(Debug, Clone)]
pub struct FaithAgent {
    pub(crate) client: Client,
}

#[napi]
impl FaithAgent {
    pub fn new() -> Result<Self, FaithError> {
        let client = Client::builder().user_agent(USER_AGENT).build()?;

        Ok(Self { client })
    }

    #[napi(constructor)]
    pub fn construct(env: Env) -> Result<Self, napi::Error> {
        Ok(Self::new().map_err(|err| err.into_js_error(&env))?)
    }
}
