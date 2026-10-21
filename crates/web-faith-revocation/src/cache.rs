//! The on-disk cache, shared by every process on the host.
//!
//! Each collection is one JSON file holding the signed changeset and its chain, so a read can
//! verify it exactly as a fetch does. CRLite filters are files named by their hash, so one
//! process's download serves the others. Every write goes to a temporary file first and is renamed
//! into place, so no reader sees a partial file.

// spec:REVOC#the-cache

use std::{
	collections::{HashMap, HashSet},
	fs, io,
	path::{Path, PathBuf},
	sync::atomic::{AtomicU64, Ordering},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::remote_settings::Signed;

/// How long a filter no signed record names is kept, so a process still on the older set is not
/// cut off. Processes check for changes daily, so a few days covers one that has been asleep.
const PRUNE_GRACE: Duration = Duration::from_secs(3 * 24 * 60 * 60);

/// How long a half-written temporary file is left alone before it counts as abandoned by a process
/// that died mid-write. A write takes moments.
const TEMP_GRACE: Duration = Duration::from_secs(60 * 60);

/// The largest cache file read back: a full CRLite filter is a few megabytes.
const MAX_FILE: u64 = 64 * 1024 * 1024;

/// A collection as cached.
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Cached {
	pub(crate) signed: Signed,
	/// When the signature was checked against the network copy, in seconds since the epoch. The
	/// chain is checked as of then on the way back in, as the signing certificates are
	/// short-lived. The cache file is not itself signed, so this is only as trustworthy as the
	/// directory: whoever can write there can pick the time, as they could already substitute any
	/// older collection that verified.
	pub(crate) verified_at: u64,
	/// When the data was last confirmed current, in seconds since the epoch.
	pub(crate) confirmed_at: u64,
}

/// The cache directory, or none, in which case nothing is read or written.
#[derive(Clone, Debug)]
pub(crate) struct Cache {
	dir: Option<PathBuf>,
}

impl Cache {
	pub(crate) fn new(dir: Option<PathBuf>) -> Self {
		Self { dir }
	}

	fn collection_path(&self, collection: &str) -> Option<PathBuf> {
		Some(self.dir.as_ref()?.join(format!("{collection}.json")))
	}

	fn filters_dir(&self) -> Option<PathBuf> {
		Some(self.dir.as_ref()?.join("filters"))
	}

	pub(crate) fn read_collection(&self, collection: &str) -> Option<Cached> {
		let bytes = read_capped(&self.collection_path(collection)?).ok()?;
		serde_json::from_slice(&bytes).ok()
	}

	pub(crate) fn write_collection(&self, collection: &str, cached: &Cached) {
		let Some(path) = self.collection_path(collection) else {
			return;
		};
		if let Ok(bytes) = serde_json::to_vec(cached) {
			// A cache that cannot be written leaves the data in memory only.
			let _ = write_atomic(&path, &bytes);
		}
	}

	pub(crate) fn read_filter(&self, hash: &[u8; 32]) -> Option<Vec<u8>> {
		read_capped(&self.filters_dir()?.join(hex::encode(hash))).ok()
	}

	pub(crate) fn write_filter(&self, hash: &[u8; 32], bytes: &[u8]) {
		if let Some(dir) = self.filters_dir() {
			let _ = write_atomic(&dir.join(hex::encode(hash)), bytes);
		}
	}

	/// Delete stored filters that `named` has not included for the grace period. When a filter
	/// first goes unnamed is kept beside the filters, so the grace period spans restarts.
	pub(crate) fn prune_filters(&self, named: &HashSet<[u8; 32]>, now: SystemTime) {
		let Some(dir) = self.filters_dir() else {
			return;
		};
		sweep_temporaries(&dir, now);
		if let Some(parent) = dir.parent() {
			sweep_temporaries(parent, now);
		}
		let Ok(entries) = fs::read_dir(&dir) else {
			return;
		};

		let ledger_path = dir.join("unnamed.json");
		let ledger: HashMap<String, u64> = read_capped(&ledger_path)
			.ok()
			.and_then(|bytes| serde_json::from_slice(&bytes).ok())
			.unwrap_or_default();
		let named: HashSet<String> = named.iter().map(hex::encode).collect();
		let now = secs(now);

		let mut next = HashMap::new();
		for entry in entries.flatten() {
			let name = entry.file_name().to_string_lossy().into_owned();
			if name.len() != 64
				|| !name.bytes().all(|b| b.is_ascii_hexdigit())
				|| named.contains(&name)
			{
				continue;
			}
			let since = ledger.get(&name).copied().unwrap_or(now);
			// On Windows a file another process has open cannot be deleted; it stays in the
			// ledger and the next update tries again.
			if now.saturating_sub(since) < PRUNE_GRACE.as_secs()
				|| fs::remove_file(entry.path()).is_err()
			{
				next.insert(name, since);
			}
		}

		if next != ledger
			&& let Ok(bytes) = serde_json::to_vec(&next)
		{
			let _ = write_atomic(&ledger_path, &bytes);
		}
	}
}

pub(crate) fn secs(time: SystemTime) -> u64 {
	time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub(crate) fn from_secs(secs: u64) -> SystemTime {
	UNIX_EPOCH + Duration::from_secs(secs)
}

fn read_capped(path: &Path) -> io::Result<Vec<u8>> {
	if fs::metadata(path)?.len() > MAX_FILE {
		return Err(io::Error::other("cache file too large"));
	}
	fs::read(path)
}

/// Delete temporary files in `dir` that a process died before renaming into place.
fn sweep_temporaries(dir: &Path, now: SystemTime) {
	let Ok(entries) = fs::read_dir(dir) else {
		return;
	};
	for entry in entries.flatten() {
		let name = entry.file_name();
		let name = name.to_string_lossy();
		if !(name.starts_with('.') && name.ends_with(".tmp")) {
			continue;
		}
		let abandoned = entry
			.metadata()
			.and_then(|meta| meta.modified())
			.ok()
			.and_then(|modified| now.duration_since(modified).ok())
			.is_some_and(|age| age > TEMP_GRACE);
		if abandoned {
			let _ = fs::remove_file(entry.path());
		}
	}
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
	static COUNTER: AtomicU64 = AtomicU64::new(0);

	let dir = path.parent().ok_or_else(|| io::Error::other("no parent"))?;
	fs::create_dir_all(dir)?;
	let tmp = dir.join(format!(
		".{}.{}.{}.tmp",
		path.file_name()
			.map(|n| n.to_string_lossy())
			.unwrap_or_default(),
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed),
	));
	if let Err(err) = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, path)) {
		let _ = fs::remove_file(&tmp);
		return Err(err);
	}
	Ok(())
}

#[cfg(test)]
mod tests;
