use rustls_pki_types::{CertificateDer, pem::PemObject};

use super::*;
use crate::test_support::{TEST_LOG_ID, TEST_SCT_TIMESTAMP, TlsPki};

fn mozilla_chain() -> Vec<CertificateDer<'static>> {
	CertificateDer::pem_slice_iter(include_bytes!("../../fixtures/www-mozilla-org-chain.pem"))
		.collect::<Result<_, _>>()
		.expect("fixture parses")
}

#[test]
fn reads_a_public_leaf() {
	let chain = mozilla_chain();
	let leaf = CertInfo::parse(&chain[0]).expect("leaf parses");
	let issuer = CertInfo::parse(&chain[1]).expect("issuer parses");

	assert_eq!(
		hex::encode(&leaf.serial),
		"0590f778e5ef7c2dae6a8135bdc04ef77b4a"
	);
	assert!(leaf.scts.len() >= 2, "a public leaf carries SCTs");
	assert_eq!(
		hex::encode(leaf.scts[0].log_id),
		"001a5d1a1c2d9375b6485578f82f71a1ae6eef397d297c8ae3157bcadee1a01e"
	);
	assert!(leaf.scts[0].timestamp > 1_700_000_000_000);

	assert_eq!(leaf.issuer, issuer.subject);
	assert_eq!(leaf.aki, issuer.ski);
	assert!(leaf.could_be_issued_by(&issuer));
	assert!(!issuer.could_be_issued_by(&leaf));
}

#[test]
fn reads_an_embedded_sct() {
	let pki = TlsPki::new(true);
	let leaf = CertInfo::parse(&pki.leaf).expect("leaf parses");
	assert_eq!(
		leaf.scts,
		vec![Sct {
			log_id: TEST_LOG_ID,
			timestamp: TEST_SCT_TIMESTAMP,
		}]
	);
	assert_eq!(leaf.serial, pki.leaf_serial);
}

#[test]
fn a_leaf_without_scts_has_none() {
	let pki = TlsPki::new(false);
	let leaf = CertInfo::parse(&pki.leaf).expect("leaf parses");
	assert!(leaf.scts.is_empty());
}

#[test]
fn a_malformed_sct_list_reads_as_none() {
	assert_eq!(parse_sct_list(&[0, 10, 0, 3, 0]), None);
	assert_eq!(parse_sct_list(&[]), None);
	assert_eq!(parse_sct_list(&[0, 0]), Some(Vec::new()));
}

#[test]
fn a_later_sct_version_is_skipped() {
	let mut sct = vec![1u8];
	sct.extend_from_slice(&[0; 40]);
	let mut list = (sct.len() as u16 + 2).to_be_bytes().to_vec();
	list.extend_from_slice(&(sct.len() as u16).to_be_bytes());
	list.extend(sct);
	assert_eq!(parse_sct_list(&list), Some(Vec::new()));
}

#[test]
fn garbage_is_not_a_certificate() {
	assert_eq!(CertInfo::parse(b"not a certificate"), None);
	assert_eq!(CertInfo::parse(&[0x30, 0x84, 0xff, 0xff, 0xff, 0xff]), None);
	assert_eq!(CertInfo::parse(&[]), None);
}

#[test]
fn a_truncated_certificate_is_not_one() {
	let chain = mozilla_chain();
	let der = chain[0].as_ref();
	for len in [1, 10, der.len() / 2, der.len() - 1] {
		assert_eq!(CertInfo::parse(&der[..len]), None, "truncated to {len}");
	}
}
