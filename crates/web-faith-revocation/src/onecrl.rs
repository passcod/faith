//! OneCRL: the intermediate and root certificates Mozilla has revoked.

// spec:REVOC#matching-onecrl

use std::collections::HashSet;

use base64::{Engine, prelude::BASE64_STANDARD};
use serde::Deserialize;

use crate::{cert::CertInfo, error::Error, links::Links};

/// The `security-state/onecrl` collection.
pub(crate) const COLLECTION: &str = "onecrl";

/// One OneCRL record. Each names a certificate one of two ways; a record naming neither is
/// ignored, as Firefox ignores it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
	issuer_name: Option<String>,
	serial_number: Option<String>,
	subject: Option<String>,
	pub_key_hash: Option<String>,
}

/// The revoked certificates, keyed both ways a record can name one.
#[derive(Debug, Default)]
pub(crate) struct OneCrl {
	issuer_serial: HashSet<(Vec<u8>, Vec<u8>)>,
	subject_key: HashSet<(Vec<u8>, Vec<u8>)>,
}

impl OneCrl {
	/// Build the set from a verified collection's records. Every record is enforced, its `enabled`
	/// flag included or not, as Firefox enforces them.
	pub(crate) fn from_records(records: &[serde_json::Value]) -> Result<Self, Error> {
		let mut set = Self::default();
		for record in records {
			let record = Record::deserialize(record)?;
			let decode = |field: &Option<String>| {
				field
					.as_deref()
					.filter(|value| !value.is_empty())
					.and_then(|value| BASE64_STANDARD.decode(value).ok())
			};

			if let (Some(issuer), Some(serial)) =
				(decode(&record.issuer_name), decode(&record.serial_number))
			{
				set.issuer_serial.insert((issuer, serial));
			} else if let (Some(subject), Some(key_hash)) =
				(decode(&record.subject), decode(&record.pub_key_hash))
			{
				set.subject_key.insert((subject, key_hash));
			}
		}
		Ok(set)
	}

	/// How many certificates the set lists.
	#[cfg(test)]
	pub(crate) fn len(&self) -> usize {
		self.issuer_serial.len() + self.subject_key.len()
	}

	pub(crate) fn lists(&self, cert: &CertInfo) -> bool {
		// Lookups by borrowed pair would need a key type of their own; the clones are a few
		// dozen bytes per certificate on the path.
		self.issuer_serial
			.contains(&(cert.issuer.clone(), cert.serial.clone()))
			|| self
				.subject_key
				.contains(&(cert.subject.clone(), cert.spki_hash.to_vec()))
	}

	/// Whether the connection is refused: every linked path holds a listed certificate.
	pub(crate) fn refuses(&self, links: &Links<'_>) -> bool {
		links.every_path_has(|cert| self.lists(cert))
	}
}

#[cfg(test)]
mod tests;
