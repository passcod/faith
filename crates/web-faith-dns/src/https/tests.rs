use std::time::Duration;

use hickory_resolver::proto::rr::{
	Name, RData, Record,
	rdata::{
		HTTPS, SVCB,
		svcb::{Alpn, SvcParamKey, SvcParamValue},
	},
};

use super::{HttpsAdvertisement, read_https_answer};

fn name(input: &str) -> Name {
	Name::from_utf8(input).unwrap()
}

/// One `HTTPS` answer, spelled the way a server would send it.
fn record(
	owner: &str,
	priority: u16,
	target: Name,
	params: Vec<(SvcParamKey, SvcParamValue)>,
	ttl: u32,
) -> Record {
	Record::from_rdata(
		name(owner),
		ttl,
		RData::HTTPS(HTTPS(SVCB::new(priority, target, params))),
	)
}

fn alpn(tokens: &[&str]) -> (SvcParamKey, SvcParamValue) {
	(
		SvcParamKey::Alpn,
		SvcParamValue::Alpn(Alpn(tokens.iter().map(|t| (*t).to_owned()).collect())),
	)
}

#[test]
fn an_h3_alpn_on_the_owner_name_advertises() {
	// spec:H3UP#advertisements-from-dns — the ordinary case: `.` as the target means the
	// owner name, so the record describes the origin itself.
	let queried = name("example.com.");
	let answers = [record(
		"example.com.",
		1,
		Name::root(),
		vec![alpn(&["h2", "h3"])],
		3600,
	)];

	assert_eq!(
		read_https_answer(&queried, &answers),
		Some(HttpsAdvertisement {
			port: None,
			ttl: Duration::from_secs(3600),
		}),
		"an `alpn` listing h3 is an advertisement, and the record's own TTL bounds it"
	);
}

#[test]
fn a_draft_h3_token_counts_like_the_header_reader_treats_one() {
	// spec:H3UP#reading-advertisements — `h3-29` is an h3-family token either way it
	// arrives, so DNS must not be stricter than the `Alt-Svc` reader.
	let queried = name("example.com.");
	let answers = [record(
		"example.com.",
		1,
		Name::root(),
		vec![alpn(&["h3-29"])],
		60,
	)];

	assert!(read_https_answer(&queried, &answers).is_some());
}

#[test]
fn a_record_without_h3_in_its_alpn_advertises_nothing() {
	// An origin that speaks only HTTP/2 says so here, and reading that as an h3
	// advertisement would send every such origin down a probe that must fail.
	let queried = name("example.com.");
	let answers = [record(
		"example.com.",
		1,
		Name::root(),
		vec![alpn(&["h2"])],
		3600,
	)];

	assert_eq!(read_https_answer(&queried, &answers), None);
}

#[test]
fn the_port_parameter_is_carried_through() {
	// spec:H3UP#advertisements-from-dns — a differing port is handled by the same
	// machinery an `Alt-Svc` advertised port is.
	let queried = name("example.com.");
	let answers = [record(
		"example.com.",
		1,
		Name::root(),
		vec![
			alpn(&["h3"]),
			(SvcParamKey::Port, SvcParamValue::Port(8443)),
		],
		3600,
	)];

	assert_eq!(
		read_https_answer(&queried, &answers).and_then(|ad| ad.port),
		Some(8443)
	);
}

#[test]
fn a_record_targeting_another_host_is_not_acted_on() {
	// Faith only upgrades to the origin's own host, exactly as it refuses an `Alt-Svc`
	// advertisement naming a different one (spec:H3UP#advertisements-from-dns).
	let queried = name("example.com.");
	let answers = [record(
		"example.com.",
		1,
		name("cdn.example.net."),
		vec![alpn(&["h3"])],
		3600,
	)];

	assert_eq!(read_https_answer(&queried, &answers), None);
}

#[test]
fn a_record_naming_the_queried_host_itself_is_acted_on() {
	// Spelling the owner name out is equivalent to the `.` shorthand, and the comparison
	// ignores case and the trailing root the way name equality should.
	let queried = name("example.com.");
	let answers = [record(
		"example.com.",
		1,
		name("ExAmPlE.CoM."),
		vec![alpn(&["h3"])],
		3600,
	)];

	assert!(read_https_answer(&queried, &answers).is_some());
}

#[test]
fn an_alias_mode_record_is_skipped() {
	// Priority 0 is AliasMode: it redirects to another name rather than describing this
	// one, and following that redirection is a resolution step this does not take.
	let queried = name("example.com.");
	let answers = [record(
		"example.com.",
		0,
		name("svc.example.net."),
		vec![alpn(&["h3"])],
		3600,
	)];

	assert_eq!(read_https_answer(&queried, &answers), None);
}

#[test]
fn the_lowest_priority_service_mode_record_wins() {
	// RFC 9460 orders ServiceMode records by ascending priority, so the most preferred
	// record is the one whose port is acted on.
	let queried = name("example.com.");
	let answers = [
		record(
			"example.com.",
			9,
			Name::root(),
			vec![
				alpn(&["h3"]),
				(SvcParamKey::Port, SvcParamValue::Port(9443)),
			],
			3600,
		),
		record(
			"example.com.",
			2,
			Name::root(),
			vec![
				alpn(&["h3"]),
				(SvcParamKey::Port, SvcParamValue::Port(2443)),
			],
			3600,
		),
	];

	assert_eq!(
		read_https_answer(&queried, &answers).and_then(|ad| ad.port),
		Some(2443),
		"the preferred record is the one acted on"
	);
}

#[test]
fn an_empty_answer_advertises_nothing() {
	// The common case for an origin with no `HTTPS` record at all: nothing learned, and
	// nothing that could make the origin probe-worthy.
	assert_eq!(read_https_answer(&name("example.com."), &[]), None);
}
