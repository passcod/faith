use super::*;

fn headers_of(request: &Request) -> Vec<(String, String)> {
	request.options.headers.clone().unwrap_or_default()
}

/// The outermost layer that set a value wins, whatever the layers beneath said.
#[test]
fn the_outermost_explicit_value_wins() {
	let inner = Request::new("https://example.com/")
		.timeout(Duration::from_secs(1))
		.build()
		.expect("a valid target");

	let outer = Request::new(inner)
		.timeout(Duration::from_secs(3))
		.build()
		.expect("layering over a request");

	assert_eq!(outer.options.timeout, Some(Duration::from_secs(3)));
}

/// A setting an outer layer does not touch is inherited unchanged.
#[test]
fn an_untouched_setting_is_inherited() {
	let inner = Request::new("https://example.com/")
		.timeout(Duration::from_secs(1))
		.method("POST")
		.build()
		.expect("a valid target");

	let outer = Request::new(inner)
		.integrity("sha256-abc")
		.build()
		.expect("layering over a request");

	assert_eq!(outer.options.timeout, Some(Duration::from_secs(1)));
	assert_eq!(outer.options.method.as_deref(), Some("POST"));
	assert_eq!(outer.options.integrity.as_deref(), Some("sha256-abc"));
}

/// The URL comes from the target at the bottom of the stack.
#[test]
fn wrapping_a_request_carries_its_url_through() {
	let inner = Request::new("https://example.com/deep/path?q=1")
		.build()
		.expect("a valid target");
	let outer = Request::new(inner)
		.method("HEAD")
		.build()
		.expect("layering");

	assert_eq!(outer.url.as_str(), "https://example.com/deep/path?q=1");
}

/// Headers merge by name: an outer value replaces, a name only set inside carries through.
#[test]
fn headers_merge_by_name() {
	let inner = Request::new("https://example.com/")
		.header("x-keep", "inner")
		.header("x-replace", "inner")
		.build()
		.expect("a valid target");

	let outer = Request::new(inner)
		.header("x-replace", "outer")
		.header("x-add", "outer")
		.build()
		.expect("layering over a request");

	let mut headers = headers_of(&outer);
	headers.sort();
	assert_eq!(
		headers,
		vec![
			("x-add".to_owned(), "outer".to_owned()),
			("x-keep".to_owned(), "inner".to_owned()),
			("x-replace".to_owned(), "outer".to_owned()),
		]
	);
}

/// Removing a name removes what the layers beneath contributed for it.
#[test]
fn removing_a_header_removes_what_is_underneath() {
	let inner = Request::new("https://example.com/")
		.header("x-gone", "inner")
		.build()
		.expect("a valid target");

	let outer = Request::new(inner)
		.remove_header("X-Gone")
		.build()
		.expect("layering over a request");

	assert!(headers_of(&outer).is_empty(), "the name was removed");
}

/// Setting the same thing twice on one builder is the same question at a smaller scale.
#[test]
fn the_later_call_wins_on_one_builder() {
	let request = Request::new("https://example.com/")
		.method("POST")
		.method("PUT")
		.build()
		.expect("a valid target");

	assert_eq!(request.options.method.as_deref(), Some("PUT"));
}

/// An `http::Request` brings its method, URL, headers, and body across.
#[test]
fn an_http_request_is_a_target() {
	let http = http::Request::builder()
		.method(http::Method::POST)
		.uri("https://example.com/submit")
		.header("x-from", "ecosystem")
		.body(Bytes::from_static(b"payload"))
		.expect("a valid http request");

	let request = Request::new(http).build().expect("it converts");

	assert_eq!(request.url.as_str(), "https://example.com/submit");
	assert_eq!(request.options.method.as_deref(), Some("POST"));
	assert_eq!(
		headers_of(&request),
		vec![("x-from".to_owned(), "ecosystem".to_owned())]
	);
	assert!(request.try_clone().is_some(), "a buffered body copies");
}

/// A layer over an `http::Request` beats what it carried, as over any other target.
#[test]
fn a_layer_beats_what_an_http_request_carried() {
	let http = http::Request::builder()
		.method(http::Method::POST)
		.uri("https://example.com/")
		.body(Bytes::new())
		.expect("a valid http request");

	let request = Request::new(http)
		.method(http::Method::PUT)
		.build()
		.expect("layering over it");

	assert_eq!(request.options.method.as_deref(), Some("PUT"));
}

/// A header name that is not one is reported where the request is resolved too.
#[test]
fn an_invalid_header_surfaces_at_build() {
	let err = Request::new("https://example.com/")
		.header("not a header name", "value")
		.build()
		.expect_err("the name is not a header name");

	assert_eq!(err.kind(), FaithErrorKind::InvalidHeader);
}

/// The first failure met is the one reported, not the last.
#[test]
fn the_first_failure_is_the_one_reported() {
	let err = Request::new("not a url")
		.header("not a header name", "value")
		.build()
		.expect_err("both are wrong");

	assert_eq!(err.kind(), FaithErrorKind::InvalidUrl);
}

/// An unparseable target is reported where the request is resolved, not by the call that took it.
#[test]
fn an_unparseable_target_surfaces_at_build() {
	let builder = Request::new("not a url");
	let err = builder.build().expect_err("the target does not parse");

	assert_eq!(err.kind(), FaithErrorKind::InvalidUrl);
}

/// A request copies when its body allows it, and reports that it cannot when it does not.
#[test]
fn try_clone_copies_what_it_can() {
	let buffered = Request::new("https://example.com/")
		.body(Bytes::from_static(b"body"))
		.build()
		.expect("a valid target");
	assert!(buffered.try_clone().is_some());

	let streamed = Request::new("https://example.com/")
		.body_stream(futures::stream::empty())
		.build()
		.expect("a valid target");
	assert!(
		streamed.try_clone().is_none(),
		"a stream is consumable once"
	);
}
