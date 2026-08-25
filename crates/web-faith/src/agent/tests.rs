use super::*;

/// An agent is constructible without a caller assembling a recipe by hand, and comes up with a
/// live client and the default user agent.
#[tokio::test]
async fn a_default_agent_comes_up() {
	let agent = Agent::new().expect("default options build an agent");

	assert!(!agent.is_closed());
	assert_eq!(agent.recipe.user_agent, crate::USER_AGENT);
	// No jar until the options ask for one.
	#[cfg(feature = "cookies")]
	assert!(agent.cookie_jar.is_none());
}

/// The builder reaches every group, and a group left alone stays absent rather than being
/// spelled out as absent.
#[cfg(all(feature = "dns", feature = "http3"))]
#[tokio::test]
async fn the_builder_sets_what_it_is_given_and_nothing_else() {
	use std::time::Duration;

	let options = Agent::builder()
		.user_agent("YourApp/1.2.3")
		.dns(|dns| dns.timeout(Duration::from_secs(2)).ndots(3))
		.pool(|pool| pool.max_idle_per_host(8))
		.into_options();

	assert_eq!(options.user_agent.as_deref(), Some("YourApp/1.2.3"));
	let dns = options.dns.expect("the dns group was reached");
	assert_eq!(dns.timeout, Some(2_000));
	assert_eq!(dns.ndots, Some(3));
	// Reached but not set stays unset...
	assert_eq!(dns.servers, None);
	// ...and a group never reached is absent entirely.
	assert!(options.tls.is_none());
	assert!(options.http3.is_none());
}

/// What the builder produces is what the agent is built from, so a built agent carries it.
#[tokio::test]
async fn the_builder_builds_an_agent() {
	let agent = Agent::builder()
		.user_agent("YourApp/1.2.3")
		.build()
		.expect("the options are valid");

	assert_eq!(agent.recipe.user_agent, "YourApp/1.2.3");
	assert!(!agent.is_closed());
}

/// A handle taken before a close still works afterwards, which is what lets a request issued
/// just before the close run to completion.
#[tokio::test]
async fn a_handle_taken_before_a_close_survives_it() {
	let agent = Agent::new().expect("default options build an agent");
	let issued = agent.client().expect("an open agent hands out a client");

	agent.close();

	assert!(agent.is_closed());
	assert!(
		agent.client().is_none(),
		"a closed agent hands out no more clients"
	);
	// The handle taken earlier is still usable; dropping it is what releases its share.
	drop(issued);
}

/// Closing acts on the agent itself, so every clone sees it.
#[tokio::test]
async fn closing_a_clone_closes_the_agent() {
	let agent = Agent::new().expect("default options build an agent");
	let clone = agent.clone();

	agent.close();

	assert!(agent.is_closed());
	assert!(clone.is_closed(), "a clone names the same agent");
}
