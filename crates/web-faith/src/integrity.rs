//! Subresource Integrity parsing and verification.
//!
//! An integrity value carries one or more digests a resource is expected to match; the resource is
//! good if it matches any one of them, and digests using an algorithm too weak to trust are
//! ignored. [`verify_integrity`] checks a body already in memory; [`integrity_checker`] with
//! [`finish_integrity`] checks one as it arrives.

// spec:SRI

use ssri::{Integrity, IntegrityChecker};

use crate::error::{FaithError, FaithErrorKind};

/// Algorithm names are compared case-insensitively, the digest itself being base64 and so not ours
/// to touch.
fn normalize_integrity(integrity: &str) -> String {
	integrity
		.split_whitespace()
		.map(|part| {
			if let Some((algo, hash)) = part.split_once('-') {
				format!("{}-{}", algo.to_ascii_lowercase(), hash)
			} else {
				part.to_string()
			}
		})
		.collect::<Vec<_>>()
		.join(" ")
}

fn parse_integrity(integrity: &str) -> Result<Integrity, FaithError> {
	let normalized = normalize_integrity(integrity);
	normalized.parse().map_err(|e| {
		FaithError::new(
			FaithErrorKind::InvalidIntegrity,
			Some(format!("failed to parse integrity value: {e}")),
		)
	})
}

/// Verify data that is already in hand.
///
/// An absent or blank value has nothing to verify and so passes.
pub fn verify_integrity(data: &[u8], integrity: &str) -> Result<(), FaithError> {
	if integrity.trim().is_empty() {
		return Ok(());
	}

	parse_integrity(integrity)?
		.check(data)
		.map_err(|_| FaithError::from(FaithErrorKind::IntegrityMismatch))?;

	Ok(())
}

/// Build a streaming integrity checker for a body read that hashes as it goes.
///
/// `None` when there is nothing to verify (no value, or an empty one), matching
/// [`verify_integrity`]. A malformed value is rejected up front with an invalid-integrity error,
/// before any of the body is touched. Feed each chunk to the returned checker with
/// [`IntegrityChecker::input`] and finish with [`finish_integrity`].
pub fn integrity_checker(integrity: Option<&str>) -> Result<Option<IntegrityChecker>, FaithError> {
	let Some(integrity) = integrity else {
		return Ok(None);
	};
	if integrity.trim().is_empty() {
		return Ok(None);
	}

	Ok(Some(IntegrityChecker::new(parse_integrity(integrity)?)))
}

/// Finish a streaming integrity check.
pub fn finish_integrity(checker: IntegrityChecker) -> Result<(), FaithError> {
	checker
		.result()
		.map(|_| ())
		.map_err(|_| FaithError::from(FaithErrorKind::IntegrityMismatch))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_sha256_valid() {
		let data = b"hello world";
		let integrity = "sha256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=";
		assert!(verify_integrity(data, integrity).is_ok());
	}

	#[test]
	fn test_sha384_valid() {
		let data = b"hello world";
		let integrity = "sha384-/b2OdaZ/KfcBpOBAOF4uI5hjA+oQI5IRr5B/y7g1eLPkF8txzmRu/QgZ3YwIjeG9";
		assert!(verify_integrity(data, integrity).is_ok());
	}

	#[test]
	fn test_sha512_valid() {
		let data = b"hello world";
		let integrity = "sha512-MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==";
		assert!(verify_integrity(data, integrity).is_ok());
	}

	#[test]
	fn test_wrong_hash() {
		let data = b"hello world";
		let integrity = "sha256-wronghashvalue";
		let result = verify_integrity(data, integrity);
		assert!(result.is_err());
		assert_eq!(result.unwrap_err().kind, FaithErrorKind::IntegrityMismatch);
	}

	#[test]
	fn test_multiple_hashes_one_correct() {
		let data = b"hello world";
		let integrity = "sha256-wronghash sha256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=";
		assert!(verify_integrity(data, integrity).is_ok());
	}

	#[test]
	fn test_multiple_hashes_all_wrong() {
		let data = b"hello world";
		let integrity = "sha256-wronghash1 sha256-wronghash2";
		let result = verify_integrity(data, integrity);
		assert!(result.is_err());
		assert_eq!(result.unwrap_err().kind, FaithErrorKind::IntegrityMismatch);
	}

	#[test]
	fn test_unknown_algorithm_skipped() {
		let data = b"hello world";
		let integrity = "sha1-ignored sha256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=";
		assert!(verify_integrity(data, integrity).is_ok());
	}

	#[test]
	fn test_empty_string() {
		let data = b"hello world";
		assert!(verify_integrity(data, "").is_ok());
		assert!(verify_integrity(data, "   ").is_ok());
	}

	#[test]
	fn test_case_insensitive_algorithm() {
		let data = b"hello world";
		let integrity = "SHA256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=";
		assert!(verify_integrity(data, integrity).is_ok());

		let integrity = "Sha256-uU0nuZNNPgilLlLX2n2r+sSE7+N6U4DukIj3rOLvzek=";
		assert!(verify_integrity(data, integrity).is_ok());
	}
}
