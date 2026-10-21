use serde_json::{Value, json};

use super::*;
use crate::{
	links::RootIndex,
	test_support::{Ca, crlite_filter, crlite_record},
};

fn filters(bytes: &[Vec<u8>]) -> Filters {
	Filters {
		filters: bytes
			.iter()
			.map(|bytes| {
				Arc::new(Filter::decode(Sha256::digest(bytes).into(), bytes).expect("decodes"))
			})
			.collect(),
	}
}

fn refused(
	filters: &Filters,
	leaf: &rustls_pki_types::CertificateDer<'_>,
	presented: &[rustls_pki_types::CertificateDer<'_>],
) -> bool {
	let roots = RootIndex::default();
	let links = Links::new(leaf, presented, Some(&roots), None).expect("leaf parses");
	filters.refuses(&links)
}

#[test]
fn reads_the_default_channel_of_the_real_collection() {
	let changeset: Value = serde_json::from_slice(include_bytes!(
		"../../fixtures/cert-revocations-changeset.json"
	))
	.expect("fixture parses");
	let records = changeset["changes"].as_array().expect("records");
	let channel = Channel::from_records(records).expect("one full filter");

	assert!(!channel.full.incremental);
	assert_eq!(channel.full.attachment.size, 6_006_419);
	assert_eq!(channel.incremental.len(), 7);
	assert!(
		channel
			.all()
			.all(|record| record.attachment.sha256().is_some())
	);
}

#[test]
fn a_channel_needs_exactly_one_full_filter() {
	let none = [crlite_record("d", b"delta", "d", true)];
	assert!(Channel::from_records(&none).is_err());

	let two = [
		crlite_record("a", b"one", "a", false),
		crlite_record("b", b"two", "b", false),
	];
	assert!(Channel::from_records(&two).is_err());

	let mut other_channel = crlite_record("c", b"other", "c", false);
	other_channel["channel"] = json!("compat");
	let one = [crlite_record("a", b"one", "a", false), other_channel];
	assert!(Channel::from_records(&one).is_ok());
}

#[test]
fn an_attachment_matches_only_its_own_bytes() {
	let record = FilterRecord::deserialize(&crlite_record("a", b"filter", "a", false))
		.expect("record parses");
	assert!(record.attachment.matches(b"filter"));
	assert!(!record.attachment.matches(b"filtex"));
	assert!(!record.attachment.matches(b"filter2"));
}

#[test]
fn a_revoked_leaf_is_refused() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let revoked = intermediate.issue_leaf(&[1, 2, 3], true, true);
	let good = intermediate.issue_leaf(&[0x10, 0, 0, 1], true, true);
	let presented = [intermediate.der.clone()];

	let set = filters(&[crlite_filter(
		&[(intermediate.spki_hash(), vec![1, 2, 3])],
		&[(intermediate.spki_hash(), vec![0x10, 0, 0, 1])],
	)]);
	assert!(refused(&set, &revoked, &presented));
	assert!(!refused(&set, &good, &presented));
}

#[test]
fn a_revocation_in_an_incremental_filter_counts() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[9], true, true);
	let presented = [intermediate.der.clone()];

	let key = intermediate.spki_hash();
	let full = crlite_filter(&[(key, vec![1])], &[(key, vec![9])]);
	let delta = crlite_filter(&[(key, vec![9])], &[(key, vec![1])]);
	assert!(refused(&filters(&[full.clone(), delta]), &leaf, &presented));
	// Without that incremental filter the revocation is simply not known.
	assert!(!refused(&filters(&[full]), &leaf, &presented));
}

#[test]
fn a_leaf_without_scts_is_not_covered() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], false, true);

	let set = filters(&[crlite_filter(&[(intermediate.spki_hash(), vec![1])], &[])]);
	assert!(!refused(
		&set,
		&leaf,
		std::slice::from_ref(&intermediate.der)
	));
}

#[test]
fn a_leaf_whose_issuer_is_unknown_is_not_covered() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], true, true);

	let set = filters(&[crlite_filter(&[(intermediate.spki_hash(), vec![1])], &[])]);
	assert!(!refused(&set, &leaf, &[]));
}

#[test]
fn an_issuer_not_enrolled_is_not_covered() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let elsewhere = root.issue_ca("Elsewhere");
	let leaf = intermediate.issue_leaf(&[1], true, true);

	let set = filters(&[crlite_filter(&[(elsewhere.spki_hash(), vec![1])], &[])]);
	assert!(!refused(
		&set,
		&leaf,
		std::slice::from_ref(&intermediate.der)
	));
}

#[test]
fn with_two_candidate_issuers_both_must_revoke() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let twin = root.issue_ca("Intermediate");
	// No authority key identifier, so both intermediates could have issued it.
	let leaf = intermediate.issue_leaf(&[5], true, false);
	let presented = [intermediate.der.clone(), twin.der.clone()];

	let one = filters(&[crlite_filter(
		&[(intermediate.spki_hash(), vec![5])],
		&[(twin.spki_hash(), vec![5])],
	)]);
	assert!(!refused(&one, &leaf, &presented));

	let both = filters(&[crlite_filter(
		&[
			(intermediate.spki_hash(), vec![5]),
			(twin.spki_hash(), vec![5]),
		],
		&[],
	)]);
	assert!(refused(&both, &leaf, &presented));
}

#[test]
fn no_filters_refuse_nothing() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], true, true);
	assert!(!refused(
		&Filters::default(),
		&leaf,
		std::slice::from_ref(&intermediate.der)
	));
}
