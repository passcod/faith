//! CRLite: leaf certificate revocations, as a full filter plus incremental filters.
//!
//! Loading and querying are split. [`Filters`] is the query side: a set of decoded filters, each
//! covering its own window of Certificate Transparency timestamps, with no knowledge of where they
//! came from. The Remote Settings records here are one way to supply them; another provider, such
//! as a system-wide cache, would hand over a [`Filters`] of its own.

// spec:REVOC#matching-crlite

use std::sync::Arc;

use clubcard_crlite::{CRLiteClubcard, CRLiteKey, CRLiteStatus, IssuerSpkiHash, LogId, Timestamp};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{cert::CertInfo, error::Error, links::Links};

/// The `security-state/cert-revocations` collection.
pub(crate) const COLLECTION: &str = "cert-revocations";

/// The channel Faith follows, the one Firefox uses by default.
const CHANNEL: &str = "default";

/// One filter's record, as signed in the collection.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct FilterRecord {
	pub(crate) incremental: bool,
	pub(crate) attachment: Attachment,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Attachment {
	/// Hex SHA-256 of the filter.
	pub(crate) hash: String,
	pub(crate) size: u64,
	/// Where the filter sits under the attachment host's base URL.
	pub(crate) location: String,
}

impl Attachment {
	/// The signed hash, decoded. `None` for a record whose hash is malformed.
	pub(crate) fn sha256(&self) -> Option<[u8; 32]> {
		hex::decode(&self.hash).ok()?.try_into().ok()
	}

	/// Whether `bytes` is the filter this record signs for.
	pub(crate) fn matches(&self, bytes: &[u8]) -> bool {
		u64::try_from(bytes.len()).is_ok_and(|len| len == self.size)
			&& self
				.sha256()
				.is_some_and(|hash| hash == <[u8; 32]>::from(Sha256::digest(bytes)))
	}
}

/// The default channel's filters, from a verified collection.
#[derive(Clone, Debug)]
pub(crate) struct Channel {
	pub(crate) full: FilterRecord,
	pub(crate) incremental: Vec<FilterRecord>,
}

impl Channel {
	/// Pick the default channel's records. Like Firefox, a channel needs exactly one full filter,
	/// and takes every incremental filter it publishes.
	pub(crate) fn from_records(records: &[serde_json::Value]) -> Result<Self, Error> {
		let mut full = Vec::new();
		let mut incremental = Vec::new();
		for record in records {
			if record.get("channel").and_then(|c| c.as_str()) != Some(CHANNEL) {
				continue;
			}
			let record = FilterRecord::deserialize(record)?;
			if record.attachment.sha256().is_none() {
				return Err(Error::Collection("malformed filter hash"));
			}
			if record.incremental {
				incremental.push(record);
			} else {
				full.push(record);
			}
		}

		match <[FilterRecord; 1]>::try_from(full) {
			Ok([full]) => Ok(Self { full, incremental }),
			Err(_) => Err(Error::Collection("expected exactly one full CRLite filter")),
		}
	}

	/// Every filter the channel names, the full one first.
	pub(crate) fn all(&self) -> impl Iterator<Item = &FilterRecord> {
		std::iter::once(&self.full).chain(&self.incremental)
	}
}

/// A decoded filter and the hash it was verified against.
pub(crate) struct Filter {
	pub(crate) hash: [u8; 32],
	pub(crate) clubcard: CRLiteClubcard,
}

impl Filter {
	pub(crate) fn decode(hash: [u8; 32], bytes: &[u8]) -> Result<Self, Error> {
		Ok(Self {
			hash,
			clubcard: CRLiteClubcard::from_bytes(bytes).map_err(|_| Error::Filter)?,
		})
	}
}

impl std::fmt::Debug for Filter {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Filter")
			.field("hash", &hex::encode(self.hash))
			.finish_non_exhaustive()
	}
}

/// The filters held, queried together: a leaf is revoked when any one of them lists it.
///
/// An incremental filter covers its own window rather than patching the full one, so a missing
/// one loses only that window's revocations and can never cause a refusal.
#[derive(Debug, Default)]
pub(crate) struct Filters {
	pub(crate) filters: Vec<Arc<Filter>>,
}

impl Filters {
	/// Whether the connection is refused: the leaf is revoked under every candidate issuer's key.
	pub(crate) fn refuses(&self, links: &Links<'_>) -> bool {
		let leaf = &links.leaf;
		if leaf.scts.is_empty() || self.filters.is_empty() {
			return false;
		}

		let mut keys: Vec<[u8; 32]> = links
			.issuers_of(leaf)
			.into_iter()
			.map(|issuer| issuer.spki_hash)
			.collect();
		keys.sort_unstable();
		keys.dedup();

		!keys.is_empty() && keys.iter().all(|key| self.revoked_under(leaf, key))
	}

	fn revoked_under(&self, leaf: &CertInfo, issuer_spki_hash: &[u8; 32]) -> bool {
		let issuer = IssuerSpkiHash(*issuer_spki_hash);
		let key = CRLiteKey::new(&issuer, &leaf.serial);
		self.filters.iter().any(|filter| {
			let timestamps = leaf
				.scts
				.iter()
				.map(|sct| (LogId(sct.log_id), Timestamp(sct.timestamp)));
			filter.clubcard.contains(&key, timestamps) == CRLiteStatus::Revoked
		})
	}
}

#[cfg(test)]
mod tests;
