use std::time::Duration;

use crate::cache::AltSvcAdvertisement;

#[test]
fn test_parse_alt_svc_simple() {
	let result = r#"h3=":443"; ma=86400"#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(result, Some(ad(443, Some(Duration::from_secs(86400)))));
}

#[test]
fn test_parse_alt_svc_no_max_age() {
	let result = r#"h3=":443""#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(result, Some(ad(443, None)));
}

#[test]
fn test_parse_alt_svc_different_port() {
	let result = r#"h3=":8443"; ma=3600"#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(result, Some(ad(8443, Some(Duration::from_secs(3600)))));
}

#[test]
fn test_parse_alt_svc_multiple_protocols() {
	let result = r#"h2=":443", h3=":443"; ma=86400"#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(result, Some(ad(443, Some(Duration::from_secs(86400)))));
}

#[test]
fn test_parse_alt_svc_h3_variant() {
	let result = r#"h3-29=":443"; ma=86400"#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(result, Some(ad(443, Some(Duration::from_secs(86400)))));
}

#[test]
fn test_parse_alt_svc_keeps_the_host() {
	let result = r#"h3="cdn.example.net:443"; ma=3600"#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(
		result,
		Some(AltSvcAdvertisement {
			host: "cdn.example.net".to_string(),
			port: 443,
			max_age: Some(Duration::from_secs(3600)),
		}),
		"the alt-authority's host must survive parsing, or a different-host \
		 advertisement looks same-host once the port matches"
	);
}

#[test]
fn test_parse_alt_svc_ipv6_host() {
	let result = r#"h3="[2001:db8::1]:8443""#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(
		result,
		Some(AltSvcAdvertisement {
			// Brackets kept: this is the form `Url::host_str` returns too, so the
			// two compare directly.
			host: "[2001:db8::1]".to_string(),
			port: 8443,
			max_age: None,
		}),
		"splitting on the last colon keeps a bracketed IPv6 literal intact"
	);
}

#[test]
fn test_parse_alt_svc_clear() {
	let result = "clear".parse::<AltSvcAdvertisement>().ok();
	assert_eq!(result, None);
}

#[test]
fn test_parse_alt_svc_no_h3() {
	let result = r#"h2=":443"; ma=86400"#.parse::<AltSvcAdvertisement>().ok();
	assert_eq!(result, None);
}

/// A same-host advertisement, the common case.
fn ad(port: u16, max_age: Option<Duration>) -> AltSvcAdvertisement {
	AltSvcAdvertisement {
		host: String::new(),
		port,
		max_age,
	}
}
