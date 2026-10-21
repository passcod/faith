use super::*;
use crate::test_support::Ca;

fn info(der: &CertificateDer<'_>) -> CertInfo {
	CertInfo::parse(der).expect("a certificate")
}

/// Whether every path from `leaf` holds one of `listed`.
fn blocked(
	leaf: &CertificateDer<'_>,
	presented: &[CertificateDer<'_>],
	roots: &[&CertificateDer<'_>],
	listed: &[&CertificateDer<'_>],
) -> bool {
	let roots = RootIndex::new(roots.iter().copied());
	let listed: Vec<CertInfo> = listed.iter().map(|der| info(der)).collect();
	Links::new(leaf, presented, Some(&roots), None)
		.expect("leaf parses")
		.every_path_has(|cert| listed.contains(cert))
}

#[test]
fn a_listed_intermediate_blocks_the_only_path() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], false, true);

	assert!(blocked(
		&leaf,
		std::slice::from_ref(&intermediate.der),
		&[&root.der],
		&[&intermediate.der]
	));
	assert!(!blocked(
		&leaf,
		std::slice::from_ref(&intermediate.der),
		&[&root.der],
		&[]
	));
}

#[test]
fn a_root_the_server_did_not_send_is_found_in_the_store() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], false, true);

	assert!(blocked(
		&leaf,
		std::slice::from_ref(&intermediate.der),
		&[&root.der],
		&[&root.der]
	));
}

#[test]
fn without_the_root_in_the_store_a_listed_root_is_not_reached() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], false, true);

	assert!(!blocked(
		&leaf,
		std::slice::from_ref(&intermediate.der),
		&[],
		&[&root.der]
	));
}

#[test]
fn a_stale_cross_sign_does_not_block_a_valid_path() {
	let old_root = Ca::root("Old root");
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let cross_sign = old_root.cross_sign(&intermediate);
	let leaf = intermediate.issue_leaf(&[1], false, true);

	// The server sends both the intermediate and its cross-sign from the old root, which is the
	// one listed. The path through the current root is clean, so the connection stands.
	let presented = [intermediate.der.clone(), cross_sign.clone()];
	assert!(!blocked(
		&leaf,
		&presented,
		&[&root.der, &old_root.der],
		&[&cross_sign]
	));

	// With the cross-sign the only way up, it blocks.
	assert!(blocked(
		&leaf,
		std::slice::from_ref(&cross_sign),
		&[&old_root.der],
		&[&cross_sign]
	));

	// And a listed old root blocks only its own path.
	assert!(!blocked(
		&leaf,
		&presented,
		&[&root.der, &old_root.der],
		&[&old_root.der]
	));
	assert!(blocked(
		&leaf,
		&presented,
		&[&root.der, &old_root.der],
		&[&old_root.der, &root.der]
	));
}

#[test]
fn a_mismatched_key_identifier_is_not_an_issuer() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	// Same subject as the intermediate, different key.
	let impostor = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], false, true);

	let presented = [intermediate.der.clone(), impostor.der.clone()];
	assert!(!blocked(&leaf, &presented, &[&root.der], &[&impostor.der]));
	assert!(blocked(
		&leaf,
		&presented,
		&[&root.der],
		&[&intermediate.der]
	));

	// A leaf that names no key identifier links to both.
	let leaf = intermediate.issue_leaf(&[1], false, false);
	assert!(!blocked(&leaf, &presented, &[&root.der], &[&impostor.der]));
	assert!(blocked(
		&leaf,
		&presented,
		&[&root.der],
		&[&impostor.der, &intermediate.der]
	));
}

#[test]
fn mutual_cross_signs_terminate() {
	let a = Ca::root("A");
	let b = Ca::root("B");
	let a_by_b = b.cross_sign(&a);
	let b_by_a = a.cross_sign(&b);
	let leaf = a.issue_leaf(&[1], false, true);

	let presented = [a_by_b.clone(), b_by_a.clone()];
	assert!(!blocked(&leaf, &presented, &[], &[]));
	assert!(blocked(&leaf, &presented, &[], &[&a_by_b]));
}

#[test]
fn an_unknown_issuer_ends_the_path_at_the_leaf() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], false, true);

	assert!(!blocked(&leaf, &[], &[&root.der], &[&intermediate.der]));
	assert!(blocked(&leaf, &[], &[&root.der], &[&leaf]));
}

#[test]
fn a_root_sent_by_the_server_and_held_is_one_candidate() {
	let root = Ca::root("Root");
	let intermediate = root.issue_ca("Intermediate");
	let leaf = intermediate.issue_leaf(&[1], false, true);
	let roots = RootIndex::new([&root.der]);
	let presented = [intermediate.der.clone(), root.der.clone()];

	let links = Links::new(&leaf, &presented, Some(&roots), None).expect("leaf parses");
	let intermediate = info(&intermediate.der);
	assert_eq!(links.issuers_of(&intermediate).len(), 1);
}
