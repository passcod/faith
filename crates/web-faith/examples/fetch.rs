//! Fetch a URL, then prepare a request once and send it more than once.
//!
//! Run with `cargo run -p web-faith --example fetch -- https://example.com/`.

use std::time::Duration;

use web_faith::{FaithError, agent::Agent, request::Request};

#[tokio::main]
async fn main() -> Result<(), FaithError> {
	let url = std::env::args()
		.nth(1)
		.unwrap_or_else(|| "https://example.com/".into());

	// An agent owns the connection pool, resolver, and caches. Cloning one is cheap and every
	// clone names the same agent, so it is what a request runs on rather than a second pool.
	let agent = Agent::builder()
		.user_agent(format!("fetch-example/1.0 {}", web_faith::USER_AGENT))
		.timeout(|timeout| timeout.total(Duration::from_secs(30)))
		.build()?;

	// `fetch` returns a builder that sends when awaited, so there is no separate send step.
	let response = agent.fetch(url.as_str()).await?;
	println!("{} {}", response.status(), response.url());
	println!("{} over {:?}", response.status_text(), response.version());

	let body = response.text().await?;
	println!("{} bytes of body", body.len());

	// A prepared request carries no agent, so it can be adjusted per call site or sent unchanged
	// on more than one agent. Layering it puts the outermost value in charge, and headers merge.
	let probe = Request::new(url.as_str())
		.method("HEAD")
		.header("x-example", "prepared")
		.build()?;

	for _ in 0..2 {
		let response = agent
			.fetch(probe.try_clone().expect("no stream body"))
			.await?;
		println!("HEAD -> {}", response.status());
	}

	// Closing releases the pool and the resolver rather than waiting for the agent to be dropped.
	// Requests already in flight run to completion; a new one is refused.
	agent.close();
	println!("closed: {}", agent.is_closed());

	Ok(())
}
