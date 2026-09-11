//! What the fetch-flavoured surface owes a caller, exercised against a real origin.
//!
//! Set `HTTPBIN_URL` to a [go-httpbin] and these run; leave it unset and they report that they
//! were skipped rather than failing, so `cargo test` works without a server to hand.
//!
//! ```console
//! $ HTTPBIN_URL=http://127.0.0.1:8888 cargo test -p web-faith
//! ```
//!
//! [go-httpbin]: https://github.com/mccutchen/go-httpbin

// spec:RSAPI spec:REQ spec:RESP

use std::time::Duration;

use serde::Deserialize;
use web_faith::{
	Agent, Request,
	error::FaithErrorKind,
	request::Priority,
	response::{FileDestination, Trailers},
};

/// The origin under test, or `None` when there is none configured.
fn origin() -> Option<String> {
	match std::env::var("HTTPBIN_URL") {
		Ok(url) if !url.is_empty() => Some(url.trim_end_matches('/').to_owned()),
		_ => None,
	}
}

/// Run `body` against the configured origin, or report the skip and return.
///
/// A macro rather than a function taking a closure: an async closure returning a future that
/// borrows its argument is more ceremony than the tests are worth.
macro_rules! against_origin {
	($origin:ident => $body:block) => {
		let Some($origin) = origin() else {
			eprintln!("skipped: set HTTPBIN_URL to run this against an origin");
			return;
		};
		$body
	};
}

fn agent() -> Agent {
	Agent::builder()
		.timeout(|timeout| timeout.total(Duration::from_secs(30)).build())
		.build()
		.expect("the options are valid")
}

/// What go-httpbin echoes back about the request it received.
///
/// Header values arrive as arrays, a name being repeatable, so `header` reads the first.
#[derive(Deserialize)]
struct Echo {
	url: String,
	#[serde(default)]
	headers: std::collections::HashMap<String, Vec<String>>,
	#[serde(default)]
	data: String,
}

impl Echo {
	fn header(&self, name: &str) -> Option<&str> {
		self.headers.get(name)?.first().map(String::as_str)
	}
}

/// A fetch builder sends when awaited, so a request is one expression.
#[tokio::test]
async fn awaiting_the_builder_sends_the_request() {
	against_origin!(origin => {
		let response = agent()
			.fetch(format!("{origin}/get"))
			.await
			.expect("the request reaches the origin");

		assert!(response.ok());
		assert_eq!(response.status(), 200);
		assert_eq!(response.status_text(), "OK");
		assert!(!response.redirected());
		assert!(!response.body_used());
	});
}

/// The body readers each consume the one body, and say so afterwards.
#[tokio::test]
async fn a_body_is_read_once_by_whichever_reader_asks() {
	against_origin!(origin => {
		let agent = agent();

		let response = agent.fetch(format!("{origin}/get")).await.expect("sent");
		let echo: Echo = response.json().await.expect("httpbin answers with json");
		assert!(echo.url.ends_with("/get"));
		assert!(response.body_used(), "reading the body marks it used");

		let response = agent.fetch(format!("{origin}/get")).await.expect("sent");
		let text = response.text().await.expect("the body is text");
		assert!(text.contains("\"url\""));

		let response = agent.fetch(format!("{origin}/get")).await.expect("sent");
		let bytes = response.bytes().await.expect("the body is bytes");
		assert_eq!(bytes.len(), text.len(), "the same body either way");

		// A second read is refused rather than returning an empty body.
		let err = response.text().await.expect_err("the body is spent");
		assert_eq!(err.kind(), FaithErrorKind::ResponseAlreadyDisturbed);
	});
}

/// A method, headers, and a body all reach the origin.
#[tokio::test]
async fn what_the_builder_sets_is_what_the_origin_sees() {
	against_origin!(origin => {
		let echo: Echo = agent()
			.fetch(format!("{origin}/post"))
			.method("POST")
			// Named, because go-httpbin echoes an unlabelled body back as a data URL.
			.header("content-type", "text/plain")
			.header("x-faith-test", "present")
			.body("the body")
			.await
			.expect("sent")
			.json()
			.await
			.expect("httpbin echoes the request");

		assert_eq!(echo.data, "the body");
		assert_eq!(echo.header("X-Faith-Test"), Some("present"));
	});
}

/// A prepared request carries no agent, so it can be sent more than once and layered over.
#[tokio::test]
async fn a_prepared_request_is_reusable_and_layerable() {
	against_origin!(origin => {
		let agent = agent();
		let prepared = Request::new(format!("{origin}/get"))
			.header("x-base", "from-the-request")
			.header("x-overridden", "from-the-request")
			.build()
			.expect("the target parses");

		// Sent unchanged, twice: the request is inert.
		for _ in 0..2 {
			let response = agent
				.fetch(prepared.try_clone().expect("no stream body"))
				.await
				.expect("sent");
			assert!(response.ok());
		}

		// Layered over: the outermost value wins, and headers merge by name.
		let echo: Echo = agent
			.fetch(prepared.try_clone().expect("no stream body"))
			.header("x-overridden", "from-the-layer")
			.header("x-added", "from-the-layer")
			.await
			.expect("sent")
			.json()
			.await
			.expect("httpbin echoes the request");

		assert_eq!(echo.header("X-Base"), Some("from-the-request"));
		assert_eq!(echo.header("X-Overridden"), Some("from-the-layer"));
		assert_eq!(echo.header("X-Added"), Some("from-the-layer"));
	});
}

/// Removing a header takes away whatever the layers beneath contributed for it.
#[tokio::test]
async fn removing_a_header_reaches_through_the_layers() {
	against_origin!(origin => {
		let prepared = Request::new(format!("{origin}/get"))
			.header("x-removed", "from-the-request")
			.build()
			.expect("the target parses");

		let echo: Echo = agent()
			.fetch(prepared)
			.remove_header("x-removed")
			.await
			.expect("sent")
			.json()
			.await
			.expect("httpbin echoes the request");

		assert!(!echo.headers.contains_key("X-Removed"));
	});
}

/// A conversion that failed on the way in is reported where the builder resolves, not at the
/// setter that took it.
#[tokio::test]
async fn a_failed_conversion_surfaces_when_the_builder_resolves() {
	let err = agent()
		.fetch("https://example.com/")
		.method("a method with spaces")
		.await
		.expect_err("the method does not convert");
	assert_eq!(err.kind(), FaithErrorKind::InvalidMethod);

	// The first failure met is the one reported, whatever follows it.
	let err = Request::new("not a url")
		.header("x-fine", "value")
		.build()
		.expect_err("the target does not parse");
	assert_eq!(err.kind(), FaithErrorKind::InvalidUrl);
}

/// A timeout shorter than the origin's delay is a `Timeout`, not a generic network failure.
#[tokio::test]
async fn a_timeout_reports_itself_as_one() {
	against_origin!(origin => {
		let err = agent()
			.fetch(format!("{origin}/delay/5"))
			.timeout(Duration::from_millis(250))
			.await
			.expect_err("the origin is slower than the deadline");
		assert_eq!(err.kind(), FaithErrorKind::Timeout);
	});
}

/// A body whose digest does not match the integrity the caller named is refused.
///
/// The check needs the whole body, so it fires where the body is read rather than where the
/// response arrives.
#[tokio::test]
async fn integrity_is_checked_against_the_body() {
	against_origin!(origin => {
		let response = agent()
			.fetch(format!("{origin}/get"))
			// A well-formed digest of the wrong bytes: 32 zero bytes, which no body hashes to.
			.integrity("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
			.await
			.expect("the response itself arrives");

		let err = response.bytes().await.expect_err("the digest cannot match");
		assert_eq!(err.kind(), FaithErrorKind::IntegrityMismatch);

		// A value naming no algorithm is refused before the body is touched at all.
		let response = agent()
			.fetch(format!("{origin}/get"))
			.integrity("not-an-integrity-value")
			.await
			.expect("the response itself arrives");
		let err = response.bytes().await.expect_err("the value does not parse");
		assert_eq!(err.kind(), FaithErrorKind::InvalidIntegrity);
	});
}

/// A closed agent refuses a new request, and every clone sees the close.
#[tokio::test]
async fn a_closed_agent_refuses_new_requests() {
	against_origin!(origin => {
		let agent = agent();
		let clone = agent.clone();

		agent
			.fetch(format!("{origin}/get"))
			.await
			.expect("the agent is open");

		agent.close();
		assert!(clone.is_closed(), "a clone names the same agent");

		let err = clone
			.fetch(format!("{origin}/get"))
			.await
			.expect_err("the agent is closed");
		assert_eq!(err.kind(), FaithErrorKind::Closed);
	});
}

/// Redirects are followed by default, and the response says which URL answered.
#[tokio::test]
async fn a_followed_redirect_reports_the_url_that_answered() {
	against_origin!(origin => {
		let response = agent()
			.fetch(format!("{origin}/redirect/2"))
			.await
			.expect("sent");

		assert!(response.ok());
		assert!(response.redirected());
		assert!(response.url().as_str().ends_with("/get"));
	});
}

/// A priority is a hint carried as a header, and one the caller wrote wins over it.
#[tokio::test]
async fn a_priority_derives_a_header_a_caller_can_override() {
	against_origin!(origin => {
		let echo: Echo = agent()
			.fetch(format!("{origin}/get"))
			.priority(Priority::High)
			.await
			.expect("sent")
			.json()
			.await
			.expect("httpbin echoes the request");
		assert_eq!(echo.header("Priority"), Some("u=1"));

		let echo: Echo = agent()
			.fetch(format!("{origin}/get"))
			.priority(Priority::High)
			.header("priority", "u=5")
			.await
			.expect("sent")
			.json()
			.await
			.expect("httpbin echoes the request");
		assert_eq!(echo.header("Priority"), Some("u=5"));
	});
}

/// Warming is advisory: it never fails, and a closed agent refuses it up front.
#[tokio::test]
async fn warming_is_advisory_but_a_closed_agent_refuses_it() {
	against_origin!(origin => {
		let agent = agent();

		agent.prefetch_dns("localhost").expect("a host to warm").await;
		agent.preconnect(&origin).expect("an origin to warm").await;

		// A string with no host is refused where the caller can see it, not by the future. The
		// future itself is not `Debug`, so the refusal is read off the `Err` rather than unwrapped.
		let Err(err) = agent.prefetch_dns("") else {
			panic!("there is no host in an empty string");
		};
		assert_eq!(err.kind(), FaithErrorKind::AddressParse);

		agent.close();
		let Err(err) = agent.prefetch_dns("localhost") else {
			panic!("a closed agent has nothing to warm");
		};
		assert_eq!(err.kind(), FaithErrorKind::Closed);
	});
}

/// The body arrives as a stream of chunks, and what only the end of the body can settle is
/// settled once the last one has been read.
#[tokio::test]
async fn a_streamed_body_settles_its_trailers_and_timing_at_the_end() {
	against_origin!(origin => {
		// A drip is chunked, so the body arrives in more than one piece.
		let response = agent()
			.fetch(format!("{origin}/drip?duration=0&numbytes=2048&delay=0"))
			.await
			.expect("sent");

		// Timing is not settled while the body is still outstanding, and the headers leg is.
		let stream = response
			.body_stream()
			.expect("the body is available")
			.expect("a drip carries a body");
		assert!(response.body_used(), "taking the stream disturbs the body");

		let mut chunks = 0;
		let mut bytes = 0;
		let mut stream = std::pin::pin!(stream);
		while let Some(chunk) = futures::StreamExt::next(&mut stream).await {
			let chunk = chunk.expect("the chunk arrives");
			chunks += 1;
			bytes += chunk.len();
		}

		assert_eq!(bytes, 2048, "every byte the origin sent arrives");
		assert!(chunks >= 1, "the body arrived in {chunks} chunk(s)");

		// Both promises settle once the body has ended, rather than hanging.
		let timing = response.timing().await;
		assert!(timing.headers_ms > 0.0);
		assert!(
			timing.body_ms.is_some(),
			"the body leg is known once the body has ended"
		);
		assert!(matches!(
			response.trailers().await,
			Trailers::None | Trailers::Some(_)
		), "the trailers promise settles rather than staying NotYet");
	});
}

/// Writing a body straight to a file reports what landed, and refuses an occupied destination
/// unless told to replace it.
#[tokio::test]
async fn writing_to_a_file_refuses_an_occupied_destination() {
	against_origin!(origin => {
		let agent = agent();
		let dir = std::env::temp_dir().join(format!("faith-write-{}", std::process::id()));
		std::fs::create_dir_all(&dir).expect("a temp directory");
		let path = dir.join("body.bin");
		let path = path.to_str().expect("a UTF-8 path");

		let mut reports = 0;
		let written = agent
			.fetch(format!("{origin}/bytes/4096"))
			.await
			.expect("sent")
			.write_to_file(path, &FileDestination::default(), |_| reports += 1)
			.await
			.expect("the destination is free");

		assert_eq!(written.bytes_written, 4096);
		assert_eq!(written.path, path, "the path written to is reported back");
		assert_eq!(
			std::fs::metadata(path).expect("the file exists").len(),
			4096,
			"and the bytes are actually on disk"
		);
		assert!(reports >= 1, "the final progress report is always delivered");

		// The default refuses an occupied destination, leaving what is there untouched.
		let err = agent
			.fetch(format!("{origin}/bytes/8"))
			.await
			.expect("sent")
			.write_to_file(path, &FileDestination::default(), |_| ())
			.await
			.expect_err("the destination is occupied");
		assert_eq!(err.kind(), FaithErrorKind::FileExists);
		assert_eq!(
			std::fs::metadata(path).expect("the file is still there").len(),
			4096,
			"a refused write leaves the original alone"
		);

		// Asked to replace it, it does.
		let written = agent
			.fetch(format!("{origin}/bytes/8"))
			.await
			.expect("sent")
			.write_to_file(
				path,
				&FileDestination {
					overwrite: true,
					..FileDestination::default()
				},
				|_| (),
			)
			.await
			.expect("the destination may be replaced");
		assert_eq!(written.bytes_written, 8);

		std::fs::remove_dir_all(&dir).expect("the temp directory goes");
	});
}

/// A response hands over as an `http::Response` whose body is the stream, undisturbed.
#[tokio::test]
async fn into_http_hands_over_the_undisturbed_body() {
	against_origin!(origin => {
		let response = agent()
			.fetch(format!("{origin}/bytes/1024"))
			.await
			.expect("sent");

		let status = response.status();
		let handed_over = response.into_http().expect("the body is undisturbed");

		assert_eq!(handed_over.status(), status, "the status carries across");
		assert!(handed_over.headers().contains_key("content-type"));

		// The body is the stream rather than a copy of it, so reading it here reads the response.
		let collected = http_body_util::BodyExt::collect(handed_over.into_body())
			.await
			.expect("the body reads")
			.to_bytes();
		assert_eq!(collected.len(), 1024);
	});
}

/// The body stream is shared rather than moved, so handing over as an `http::Response` leaves an
/// earlier stream still readable, and both see the whole body.
#[tokio::test]
async fn the_body_stream_is_shared_between_its_consumers() {
	against_origin!(origin => {
		let response = agent()
			.fetch(format!("{origin}/bytes/64"))
			.await
			.expect("sent");

		let taken = response
			.body_stream()
			.expect("the body is available")
			.expect("a body");

		// Handing over is not refused by the stream already having been taken: both hand out the
		// same shared stream rather than one moving it away from the other.
		let handed_over = response.into_http().expect("the body is shared, not moved");
		let collected = http_body_util::BodyExt::collect(handed_over.into_body())
			.await
			.expect("the body reads")
			.to_bytes();
		assert_eq!(collected.len(), 64);

		let mut taken = std::pin::pin!(taken);
		let mut bytes = 0;
		while let Some(chunk) = futures::StreamExt::next(&mut taken).await {
			bytes += chunk.expect("the chunk arrives").len();
		}
		assert_eq!(bytes, 64, "the earlier stream still sees the whole body");
	});
}
