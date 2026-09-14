//! The public surface a Rust caller actually reaches for.
//!
//! Compiling is most of the test: every path here is one a caller writes, so a type that stops
//! being nameable from outside the crate fails this rather than passing unnoticed.

use std::time::Duration;

use web_faith::{
	Agent,
	agent::{AgentOptionsBuilder, AgentStats, Header, RedirectPolicy},
};

#[tokio::test]
async fn the_builder_reaches_every_option_group() {
	let _: AgentOptionsBuilder = Agent::builder();

	let builder = Agent::builder()
		.user_agent("Surface/1.0")
		.redirect(RedirectPolicy::Stop)
		.headers([Header::builder().name("x-surface").value("1").build()])
		.timeout(|timeout| timeout.connect(Duration::from_secs(2)).build())
		.pool(|pool| pool.max_idle_per_host(8).build())
		.flow_control(|flow| flow.stream_window(1024 * 1024).build())
		.quirks(|quirks| quirks.h1_request_streaming(true).build())
		.tls(|tls| tls.required(true).build());

	#[cfg(feature = "dns")]
	let builder = builder.dns(|dns| dns.ndots(2).build());

	#[cfg(feature = "cache")]
	let builder = builder.cache(|cache| cache.store(web_faith::agent::CacheStore::Memory).build());

	let agent = builder.build().expect("an agent from the builder");

	let _: AgentStats = agent.stats();
	agent.close();
}
