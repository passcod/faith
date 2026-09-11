//! Reading what a warm-up was asked to warm.
//!
//! Warming a name and warming an origin take their arguments loosely, so the parsing that decides
//! what was meant is worth keeping in one place, away from the verbs that act on it.

use url::Url;

// spec:WARM
/// Extract the bare host from a DNS prefetch argument, ignoring any scheme, port, or path, or
/// `None` if there is no host to resolve. A DNS name carries none of those parts, so a fuller
/// string is reduced to its host.
pub fn extract_host(input: &str) -> Option<String> {
	// A string that already spells a scheme is read as the URL it is; anything else is the
	// bare-host case, where a name is not a URL on its own and giving it an authority makes it
	// parse as one. Telling the two apart on the scheme separator matters both ways: `example.com:8443`
	// otherwise parses as a *scheme* of `example.com` with no host, and a schemed string with no host
	// (`file:///path`, a bare `https://`) would have its scheme misread as a host by the fallback.
	let url = if input.contains("://") {
		Url::parse(input).ok()?
	} else {
		Url::parse(&format!("dns://{input}")).ok()?
	};
	let host = url.host_str()?;
	// `host_str` brackets an IPv6 literal; the resolver wants it bare.
	let host = host
		.strip_prefix('[')
		.and_then(|rest| rest.strip_suffix(']'))
		.unwrap_or(host);
	(!host.is_empty()).then(|| host.to_owned())
}

/// Reduce a preconnect argument to its origin, or `None` if it is not a connectable origin. Path,
/// query, fragment, and userinfo are stripped — the same reduction the HTTP/3 probe applies — and
/// the scheme must have a known default port so an omitted port resolves.
pub fn reduce_to_origin(input: &str) -> Option<Url> {
	let mut url = Url::parse(input).ok()?;
	if !url.has_host() || url.port_or_known_default().is_none() {
		return None;
	}
	url.set_path("/");
	url.set_query(None);
	url.set_fragment(None);
	let _ = url.set_username("");
	let _ = url.set_password(None);
	Some(url)
}

/// The `scheme://host:port` key an origin coalesces on, with the port defaulted by scheme so
/// `https://host` and `https://host:443` are the same origin. Matches the Alt-Svc cache's key.
pub fn origin_key(url: &Url) -> String {
	format!(
		"{}://{}:{}",
		url.scheme(),
		url.host_str().unwrap_or_default(),
		url.port_or_known_default().unwrap_or_default(),
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn prefetch_dns_takes_a_bare_host() {
		assert_eq!(extract_host("example.com").as_deref(), Some("example.com"));
	}

	#[test]
	fn prefetch_dns_ignores_the_parts_a_name_does_not_have() {
		// A DNS name has no scheme, port, or path, so a fuller string is reduced to its host
		// rather than rejected (spec:WARM#dns-prefetch).
		for input in [
			"https://example.com",
			"https://example.com:8443",
			"https://example.com/some/path?q=1#frag",
			"https://user:pass@example.com/",
			"example.com:8443",
		] {
			assert_eq!(
				extract_host(input).as_deref(),
				Some("example.com"),
				"{input:?} names example.com whatever else it carries"
			);
		}
	}

	#[test]
	fn prefetch_dns_unwraps_an_ipv6_literal() {
		// `host_str` brackets an IPv6 literal, but the resolver wants it bare.
		assert_eq!(
			extract_host("https://[2001:db8::1]:8443").as_deref(),
			Some("2001:db8::1")
		);
	}

	#[test]
	fn prefetch_dns_rejects_a_string_with_no_host() {
		for input in ["", "   ", "/just/a/path", "https://"] {
			assert!(
				extract_host(input).is_none(),
				"{input:?} names no host to resolve"
			);
		}
	}

	#[test]
	fn preconnect_reduces_a_longer_url_to_its_origin() {
		// The same reduction the HTTP/3 probe applies (spec:WARM#preconnect).
		let url = reduce_to_origin("https://user:pass@example.com/some/path?q=1#frag")
			.expect("a full URL reduces to its origin");

		assert_eq!(url.as_str(), "https://example.com/");
		assert_eq!(url.username(), "", "userinfo is stripped");
		assert_eq!(url.password(), None);
		assert_eq!(url.query(), None);
		assert_eq!(url.fragment(), None);
	}

	#[test]
	fn preconnect_defaults_the_port_by_scheme() {
		// An omitted port defaults by scheme, so an origin spelled either way coalesces on one
		// key (spec:WARM#preconnect).
		for (bare, spelled) in [
			("https://example.com", "https://example.com:443"),
			("http://example.com", "http://example.com:80"),
		] {
			let bare = origin_key(&reduce_to_origin(bare).expect("parses"));
			let spelled = origin_key(&reduce_to_origin(spelled).expect("parses"));
			assert_eq!(
				bare, spelled,
				"the omitted port defaults to the spelled one"
			);
		}
	}

	#[test]
	fn preconnect_keeps_distinct_origins_apart() {
		// The pool caps and the warm record are per origin: scheme, host, and port together
		// (spec:POOL).
		let key = |input: &str| origin_key(&reduce_to_origin(input).expect("parses"));

		assert_ne!(key("https://example.com"), key("https://example.com:8443"));
		assert_ne!(key("https://example.com"), key("http://example.com"));
		assert_ne!(key("https://example.com"), key("https://other.example"));
	}

	#[test]
	fn preconnect_rejects_what_cannot_be_connected_to() {
		for input in [
			"not an origin",
			"",
			"/just/a/path",
			// No host to connect to.
			"file:///etc/hosts",
			// No default port for the scheme, and none given.
			"unknownscheme://example.com",
		] {
			assert!(
				reduce_to_origin(input).is_none(),
				"{input:?} is not a connectable origin"
			);
		}
	}
}
