use hickory_resolver::proto::rr::Name;

use super::exempt_suffixes;

#[test]
fn a_root_suffix_never_exempts_everything() {
	// A Windows host with no DNS domain reports the root as its domain, and the root is the
	// parent of every name. Taking it as a suffix exempted every lookup and sent it to the
	// system resolver, leaving `dns.servers` configured and unused (spec:DNS#exempt-names).
	let suffixes = exempt_suffixes(vec![Name::root()], &[]);
	assert!(
		!suffixes.iter().any(|suffix| suffix.is_root()),
		"the root is not admitted as a suffix"
	);

	let name = Name::from_utf8("nonexistent.example").unwrap();
	assert!(
		!suffixes.iter().any(|suffix| suffix.zone_of(&name)),
		"so an ordinary name is not exempt and reaches the configured servers"
	);

	// The names that must stay exempt still are, and a real system suffix still counts.
	let suffixes = exempt_suffixes(
		vec![Name::root(), Name::from_utf8("corp.example").unwrap()],
		&[],
	);
	for exempt in ["localhost", "printer.local", "host.corp.example"] {
		let name = Name::from_utf8(exempt).unwrap();
		assert!(
			suffixes.iter().any(|suffix| suffix.zone_of(&name)),
			"{exempt} is exempt"
		);
	}
}

#[test]
fn a_root_entry_from_the_caller_is_refused_too() {
	// Whichever list it arrives in, the root would disable the caller's own servers.
	let suffixes = exempt_suffixes(vec![], &[Name::root()]);
	let name = Name::from_utf8("nonexistent.example").unwrap();
	assert!(!suffixes.iter().any(|suffix| suffix.zone_of(&name)));
}

#[test]
fn exempt_matches_a_suffix_exactly_or_as_a_subdomain() {
	// spec:DNS#exempt-names
	let local = Name::from_ascii("local").unwrap();
	assert!(local.zone_of(&Name::from_utf8("printer.local").unwrap()));
	assert!(local.zone_of(&Name::from_utf8("local").unwrap()));
	assert!(!local.zone_of(&Name::from_utf8("mylocal.example").unwrap()));
}
