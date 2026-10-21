//! The Remote Settings HTTP API, as far as these collections need it.

// spec:REVOC#keeping-current

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
	error::Error,
	signature::{self, Trust},
};

/// The bucket both collections live in.
pub(crate) const BUCKET: &str = "security-state";

/// The largest JSON document accepted: OneCRL is under a megabyte.
const MAX_JSON: usize = 16 * 1024 * 1024;

/// The largest certificate chain accepted.
const MAX_CHAIN: usize = 64 * 1024;

/// A collection as served, with what is needed to check its signature again later.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Changeset {
	pub(crate) metadata: Metadata,
	pub(crate) timestamp: u64,
	pub(crate) changes: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Metadata {
	pub(crate) signature: Signature,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Signature {
	pub(crate) x5u: String,
	pub(crate) signature: String,
}

/// A changeset whose signature has been checked, with the chain that checked it, so the same check
/// can be repeated on the copy read back from the cache.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Signed {
	pub(crate) changeset: Changeset,
	pub(crate) chain: String,
}

impl Signed {
	/// Check the signature, at `time` for the chain's validity, and that the collection is not
	/// older than `held`.
	pub(crate) fn verify(
		&self,
		trust: &Trust,
		time: rustls_pki_types::UnixTime,
		held: Option<u64>,
	) -> Result<(), Error> {
		if held.is_some_and(|held| self.changeset.timestamp < held) {
			return Err(Error::Rollback);
		}
		let message = signature::signed_message(&self.changeset.changes, self.changeset.timestamp);
		signature::verify(
			trust,
			&message,
			&self.changeset.metadata.signature.signature,
			self.chain.as_bytes(),
			time,
		)
	}

	/// The records, tombstones left out.
	pub(crate) fn records(&self) -> Vec<Value> {
		self.changeset
			.changes
			.iter()
			.filter(|record| record.get("deleted") != Some(&Value::Bool(true)))
			.cloned()
			.collect()
	}
}

#[derive(Debug, Deserialize)]
struct Changes {
	changes: Vec<Change>,
}

#[derive(Debug, Deserialize)]
struct Change {
	bucket: String,
	collection: String,
	last_modified: u64,
}

#[derive(Debug, Deserialize)]
struct ServerInfo {
	capabilities: Capabilities,
}

#[derive(Debug, Deserialize)]
struct Capabilities {
	attachments: AttachmentsCapability,
}

#[derive(Debug, Deserialize)]
struct AttachmentsCapability {
	base_url: String,
}

/// A Remote Settings server.
#[derive(Clone, Debug)]
pub(crate) struct RemoteSettings {
	client: Client,
	base_url: Url,
}

impl RemoteSettings {
	pub(crate) fn new(client: Client, base_url: Url) -> Self {
		Self { client, base_url }
	}

	fn url(&self, path: &str) -> Result<Url, Error> {
		self.base_url
			.join(path)
			.map_err(|_| Error::Collection("unusable base URL"))
	}

	/// When each of `collections` last changed, by the server's change feed.
	pub(crate) async fn last_modified(
		&self,
		collections: &[&str],
	) -> Result<Vec<Option<u64>>, Error> {
		let url = self.url("buckets/monitor/collections/changes/changeset?_expected=0")?;
		let changes: Changes = serde_json::from_slice(&self.get(url, MAX_JSON).await?)?;
		Ok(collections
			.iter()
			.map(|collection| {
				changes
					.changes
					.iter()
					.find(|change| change.bucket == BUCKET && change.collection == *collection)
					.map(|change| change.last_modified)
			})
			.collect())
	}

	/// A collection's records and signature, with its signing chain.
	pub(crate) async fn changeset(&self, collection: &str, expected: u64) -> Result<Signed, Error> {
		let url = self.url(&format!(
			"buckets/{BUCKET}/collections/{collection}/changeset?_expected={expected}"
		))?;
		let changeset: Changeset = serde_json::from_slice(&self.get(url, MAX_JSON).await?)?;

		let x5u = Url::parse(&changeset.metadata.signature.x5u)
			.map_err(|_| Error::Signature("malformed x5u URL"))?;
		let chain = String::from_utf8(self.get(x5u, MAX_CHAIN).await?.to_vec())
			.map_err(|_| Error::Signature("malformed certificate chain"))?;

		Ok(Signed { changeset, chain })
	}

	/// Where the server says attachments are downloaded from.
	pub(crate) async fn attachments_base(&self) -> Result<Url, Error> {
		let info: ServerInfo = serde_json::from_slice(&self.get(self.url("")?, MAX_JSON).await?)?;
		Url::parse(&info.capabilities.attachments.base_url)
			.map_err(|_| Error::Collection("malformed attachments base URL"))
	}

	/// Download an attachment of at most `size` bytes.
	pub(crate) async fn attachment(
		&self,
		base: &Url,
		location: &str,
		size: u64,
	) -> Result<Vec<u8>, Error> {
		let url = base
			.join(location)
			.map_err(|_| Error::Collection("malformed attachment location"))?;
		let cap = usize::try_from(size).map_err(|_| Error::TooLarge)?;
		self.get(url, cap).await
	}

	/// GET `url`, refusing a body over `cap` bytes without reading the rest of it.
	async fn get(&self, url: Url, cap: usize) -> Result<Vec<u8>, Error> {
		let mut response = self.client.get(url).send().await?.error_for_status()?;
		if response
			.content_length()
			.is_some_and(|len| len > cap as u64)
		{
			return Err(Error::TooLarge);
		}

		let mut body = Vec::new();
		while let Some(chunk) = response.chunk().await? {
			if body.len() + chunk.len() > cap {
				return Err(Error::TooLarge);
			}
			body.extend_from_slice(&chunk);
		}
		Ok(body)
	}
}
