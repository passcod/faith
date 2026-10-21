//! Remote Settings content signatures.
//!
//! A collection is signed as a whole: the canonical JSON of its records and timestamp, prefixed
//! with `Content-Signature:\0`, under P-384 ECDSA with SHA-384. The signing certificate comes as a
//! PEM chain from the signature's `x5u` URL, and must lead to a pinned root and name the expected
//! signer.

// spec:REVOC#integrity

use std::{fmt::Write, sync::Arc};

use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use rustls::{SignatureScheme, crypto::CryptoProvider};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime, pem::PemObject};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::Error;

/// The code-signing extended key usage, which content-signing certificates carry.
const EKU_CODE_SIGNING: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x03];

/// What a signature is checked against.
#[derive(Clone, Debug)]
pub(crate) struct Trust {
	pub(crate) root_hash: [u8; 32],
	pub(crate) signer: ServerName<'static>,
	pub(crate) provider: Arc<CryptoProvider>,
}

/// The message a collection's signature covers.
pub(crate) fn signed_message(records: &[Value], timestamp: u64) -> Vec<u8> {
	let mut records: Vec<&Value> = records
		.iter()
		.filter(|record| record.get("deleted") != Some(&Value::Bool(true)))
		.collect();
	records.sort_by(|a, b| id_of(a).cmp(id_of(b)));

	let mut out = String::from("Content-Signature:\0{\"data\":[");
	for (i, record) in records.into_iter().enumerate() {
		if i > 0 {
			out.push(',');
		}
		canonical(record, &mut out);
	}
	write!(out, "],\"last_modified\":\"{timestamp}\"}}").expect("writing to a String");
	out.into_bytes()
}

fn id_of(record: &Value) -> &str {
	record.get("id").and_then(Value::as_str).unwrap_or_default()
}

/// Canonical JSON as Remote Settings signs it: keys sorted, no whitespace, and non-ASCII escaped.
fn canonical(value: &Value, out: &mut String) {
	match value {
		Value::Null => out.push_str("null"),
		Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
		Value::Number(n) => match n.as_f64() {
			// JavaScript prints an integral float without a fraction.
			Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e21 => {
				write!(out, "{}", f as i64).expect("writing to a String")
			}
			_ => write!(out, "{n}").expect("writing to a String"),
		},
		Value::String(s) => escape(s, out),
		Value::Array(items) => {
			out.push('[');
			for (i, item) in items.iter().enumerate() {
				if i > 0 {
					out.push(',');
				}
				canonical(item, out);
			}
			out.push(']');
		}
		Value::Object(map) => {
			let mut keys: Vec<&String> = map.keys().collect();
			// JavaScript orders keys by UTF-16 code unit.
			keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
			out.push('{');
			for (i, key) in keys.into_iter().enumerate() {
				if i > 0 {
					out.push(',');
				}
				escape(key, out);
				out.push(':');
				canonical(&map[key], out);
			}
			out.push('}');
		}
	}
}

fn escape(s: &str, out: &mut String) {
	out.push('"');
	for c in s.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\u{8}' => out.push_str("\\b"),
			'\u{c}' => out.push_str("\\f"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			' '..='~' => out.push(c),
			_ => {
				let mut units = [0u16; 2];
				for unit in c.encode_utf16(&mut units) {
					write!(out, "\\u{unit:04x}").expect("writing to a String");
				}
			}
		}
	}
	out.push('"');
}

/// Verify `signature` (base64url, raw `r‖s`) over `message`, with the leaf of the PEM `chain`.
///
/// The chain must end at the pinned root, and its leaf must be valid at `time`, carry the
/// code-signing usage, and name the signer.
pub(crate) fn verify(
	trust: &Trust,
	message: &[u8],
	signature: &str,
	chain: &[u8],
	time: UnixTime,
) -> Result<(), Error> {
	let certs: Vec<CertificateDer<'_>> = CertificateDer::pem_slice_iter(chain)
		.collect::<Result<_, _>>()
		.map_err(|_| Error::Signature("malformed certificate chain"))?;
	let [leaf, intermediates @ .., root] = certs.as_slice() else {
		return Err(Error::Signature("certificate chain too short"));
	};

	if <[u8; 32]>::from(Sha256::digest(root)) != trust.root_hash {
		return Err(Error::Signature("chain does not end at the pinned root"));
	}
	let anchor = webpki::anchor_from_trusted_cert(root)
		.map_err(|_| Error::Signature("malformed root certificate"))?;
	// As in Firefox, the signing certificate itself must carry the code-signing usage, and a
	// certificate above it need only allow it.
	if !crate::cert::has_extended_key_usage(leaf, EKU_CODE_SIGNING) {
		return Err(Error::Signature(
			"signing certificate is not for code signing",
		));
	}
	let leaf = webpki::EndEntityCert::try_from(leaf)
		.map_err(|_| Error::Signature("malformed signing certificate"))?;

	let algorithms = &trust.provider.signature_verification_algorithms;
	leaf.verify_for_usage(
		algorithms.all,
		&[anchor],
		intermediates,
		time,
		webpki::KeyUsage::required_if_present(EKU_CODE_SIGNING),
		None,
		None,
	)
	.map_err(|_| Error::Signature("signing certificate does not chain to the root"))?;
	leaf.verify_is_valid_for_subject_name(&trust.signer)
		.map_err(|_| Error::Signature("signing certificate names another signer"))?;

	let raw = BASE64_URL_SAFE_NO_PAD
		.decode(signature.trim_end_matches('='))
		.map_err(|_| Error::Signature("malformed signature"))?;
	let der = raw_to_der(&raw).ok_or(Error::Signature("malformed signature"))?;

	let (_, p384) = algorithms
		.mapping
		.iter()
		.find(|(scheme, _)| *scheme == SignatureScheme::ECDSA_NISTP384_SHA384)
		.ok_or(Error::Signature("crypto provider lacks P-384"))?;
	if p384
		.iter()
		.any(|alg| leaf.verify_signature(*alg, message, &der).is_ok())
	{
		Ok(())
	} else {
		Err(Error::Signature("signature does not match"))
	}
}

/// A raw P-384 `r‖s` signature as the DER `ECDSA-Sig-Value` webpki takes.
pub(crate) fn raw_to_der(raw: &[u8]) -> Option<Vec<u8>> {
	if raw.len() != 96 {
		return None;
	}
	let (r, s) = raw.split_at(48);
	let mut body = Vec::with_capacity(102);
	for int in [r, s] {
		let int = match int.iter().position(|&b| b != 0) {
			Some(start) => &int[start..],
			None => &int[47..],
		};
		let pad = int[0] & 0x80 != 0;
		body.push(0x02);
		body.push((int.len() + usize::from(pad)) as u8);
		if pad {
			body.push(0);
		}
		body.extend_from_slice(int);
	}
	let mut der = vec![0x30, body.len() as u8];
	der.extend(body);
	Some(der)
}

#[cfg(test)]
mod tests;
