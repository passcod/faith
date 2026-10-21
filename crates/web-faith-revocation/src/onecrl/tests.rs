use serde_json::json;

use super::*;
use crate::{
	links::RootIndex,
	test_support::{Ca, onecrl_issuer_serial, onecrl_subject_key},
};

fn info(der: &rustls_pki_types::CertificateDer<'_>) -> CertInfo {
	CertInfo::parse(der).expect("a certificate")
}

#[test]
fn matches_by_issuer_and_serial() {
	let root = Ca::root("Root");
	let listed = root.issue_ca("Listed");
	let other = root.issue_ca("Other");

	let set = OneCrl::from_records(&[onecrl_issuer_serial(&listed.der)]).expect("records parse");
	assert_eq!(set.len(), 1);
	assert!(set.lists(&info(&listed.der)));
	assert!(!set.lists(&info(&other.der)));
}

#[test]
fn matches_by_subject_and_key_hash() {
	let root = Ca::root("Root");
	let listed = root.issue_ca("Listed");
	let other = root.issue_ca("Other");
	// Same subject and key as the listed one, under a different issuer: still listed.
	let cross_sign = Ca::root("Elsewhere").cross_sign(&listed);

	let set = OneCrl::from_records(&[onecrl_subject_key(&listed.der)]).expect("records parse");
	assert!(set.lists(&info(&listed.der)));
	assert!(set.lists(&info(&cross_sign)));
	assert!(!set.lists(&info(&other.der)));
}

#[test]
fn a_disabled_record_is_enforced() {
	let root = Ca::root("Root");
	let listed = root.issue_ca("Listed");
	let mut record = onecrl_issuer_serial(&listed.der);
	record["enabled"] = json!(false);

	let set = OneCrl::from_records(&[record]).expect("records parse");
	assert!(set.lists(&info(&listed.der)));
}

#[test]
fn records_naming_nothing_usable_are_ignored() {
	let set = OneCrl::from_records(&[
		json!({ "id": "a", "issuerName": "not base64!", "serialNumber": "AQ==" }),
		json!({ "id": "b", "issuerName": "", "serialNumber": "" }),
		json!({ "id": "c", "details": { "why": "nothing" } }),
		json!({ "id": "d", "subject": "AQ==" }),
	])
	.expect("records parse");
	assert_eq!(set.len(), 0);
}

#[test]
fn refuses_only_when_every_path_is_listed() {
	let old_root = Ca::root("Old root");
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let cross_sign = old_root.cross_sign(&intermediate);
	let leaf = intermediate.issue_leaf(&[1], false, true);
	let roots = RootIndex::new([&root.der, &old_root.der]);
	let presented = [intermediate.der.clone(), cross_sign.clone()];
	let links = Links::new(&leaf, &presented, Some(&roots), None).expect("leaf parses");

	let cross_listed = OneCrl::from_records(&[onecrl_issuer_serial(&cross_sign)]).expect("parses");
	assert!(!cross_listed.refuses(&links));

	let key_listed =
		OneCrl::from_records(&[onecrl_subject_key(&intermediate.der)]).expect("parses");
	assert!(key_listed.refuses(&links), "the key is on both paths");
}
