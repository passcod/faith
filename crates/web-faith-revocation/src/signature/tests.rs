use std::time::Duration;

use serde_json::json;

use super::*;
use crate::{
	MOZILLA_ROOT_HASH, MOZILLA_SIGNER,
	remote_settings::{Changeset, Signed},
	test_support::{SigningPki, provider},
};

fn mozilla() -> Trust {
	Trust {
		root_hash: MOZILLA_ROOT_HASH,
		signer: ServerName::try_from(MOZILLA_SIGNER).expect("a valid name"),
		provider: provider(),
	}
}

fn real() -> Signed {
	Signed {
		changeset: serde_json::from_slice::<Changeset>(include_bytes!(
			"../../fixtures/cert-revocations-changeset.json"
		))
		.expect("fixture parses"),
		chain: include_str!("../../fixtures/content-signature-chain.pem").to_owned(),
	}
}

fn at_ms(ms: u64) -> UnixTime {
	UnixTime::since_unix_epoch(Duration::from_millis(ms))
}

#[test]
fn a_real_collection_verifies_against_mozillas_root() {
	let signed = real();
	let when = at_ms(signed.changeset.timestamp);
	signed.verify(&mozilla(), when, None).expect("verifies");
}

#[test]
fn a_tampered_real_collection_does_not() {
	let mut signed = real();
	let when = at_ms(signed.changeset.timestamp);
	signed.changeset.changes.pop();
	assert!(signed.verify(&mozilla(), when, None).is_err());

	let mut signed = real();
	signed.changeset.timestamp += 1;
	assert!(signed.verify(&mozilla(), when, None).is_err());
}

#[test]
fn the_chain_must_be_valid_at_the_time() {
	let signed = real();
	// Long after the short-lived signing certificate expired.
	let later = at_ms(signed.changeset.timestamp + 400 * 24 * 3600 * 1000);
	assert!(signed.verify(&mozilla(), later, None).is_err());
}

#[test]
fn the_chain_must_end_at_the_pinned_root() {
	let signed = real();
	let when = at_ms(signed.changeset.timestamp);
	let mut trust = mozilla();
	trust.root_hash[0] ^= 1;
	assert!(signed.verify(&trust, when, None).is_err());
}

#[test]
fn the_signing_certificate_must_name_the_signer() {
	let pki = SigningPki::with_signer("someone-else.content-signature.mozilla.org");
	let records = vec![json!({ "id": "a", "last_modified": 1 })];
	let signed = Signed {
		changeset: Changeset {
			metadata: crate::remote_settings::Metadata {
				signature: crate::remote_settings::Signature {
					x5u: String::new(),
					signature: pki.sign(&records, 5),
				},
			},
			timestamp: 5,
			changes: records,
		},
		chain: pki.chain_pem().to_owned(),
	};
	let mut trust = mozilla();
	trust.root_hash = pki.root_hash();
	assert!(signed.verify(&trust, UnixTime::now(), None).is_err());

	trust.signer =
		ServerName::try_from("someone-else.content-signature.mozilla.org").expect("a valid name");
	signed
		.verify(&trust, UnixTime::now(), None)
		.expect("verifies for its own signer");
}

#[test]
fn an_older_collection_is_refused() {
	let signed = real();
	let when = at_ms(signed.changeset.timestamp);
	let held = signed.changeset.timestamp;
	signed
		.verify(&mozilla(), when, Some(held))
		.expect("the same one is fine");
	assert!(matches!(
		signed.verify(&mozilla(), when, Some(held + 1)),
		Err(Error::Rollback)
	));
}

#[test]
fn canonical_json_sorts_keys_and_escapes_non_ascii() {
	let records = vec![
		json!({ "id": "b", "z": 1, "a": [true, null, 1.0, -2, 0.5], "last_modified": 2 }),
		json!({ "id": "a", "s": "é\u{1F600}\"\\\n\u{1}~", "last_modified": 1 }),
		json!({ "id": "c", "deleted": true }),
	];
	let message = String::from_utf8(signed_message(&records, 42)).expect("ASCII");
	assert_eq!(
		message,
		"Content-Signature:\0{\"data\":[\
		{\"id\":\"a\",\"last_modified\":1,\"s\":\"\\u00e9\\ud83d\\ude00\\\"\\\\\\n\\u0001~\"},\
		{\"a\":[true,null,1,-2,0.5],\"id\":\"b\",\"last_modified\":2,\"z\":1}\
		],\"last_modified\":\"42\"}"
	);
}

#[test]
fn raw_signatures_convert_to_der() {
	let mut raw = vec![0u8; 96];
	raw[0] = 0x80; // r needs a leading zero
	raw[95] = 1; // s is tiny
	let der = raw_to_der(&raw).expect("converts");
	assert_eq!(&der[..4], &[0x30, 54, 0x02, 49]);
	assert_eq!(&der[der.len() - 3..], &[0x02, 1, 1]);
	assert_eq!(raw_to_der(&raw[..95]), None);
}
