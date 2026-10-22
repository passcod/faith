//! Reading upki's cache into filters.
//!
//! upki writes the filters, then `index.bin`, then `manifest.json`, each by atomic replacement,
//! and only then removes filters nothing names. A reader that finds a named filter missing has
//! therefore raced an update, and reads again from the start.

// spec:REVOC#a-usable-cache

use std::{
	collections::{HashMap, HashSet},
	fs::{self, File},
	io::{self, Read},
	path::Path,
	sync::Arc,
	time::SystemTime,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::index::{self, MAX_INDEX};
use crate::{
	cache::MAX_FILE,
	crlite::{Filter, Filters},
	error::Error,
};

/// The largest `manifest.json` read. Real ones are a kilobyte or two.
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;

/// How many times a read that finds a filter missing starts again before giving up.
const ATTEMPTS: usize = 2;

/// The parts of upki's `manifest.json` Faith uses.
#[derive(Deserialize)]
struct Manifest {
	files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
	filename: String,
	size: u64,
	/// Hex SHA-256 of the file.
	hash: String,
}

/// A usable upki cache, read.
#[derive(Debug)]
pub(crate) struct Snapshot {
	pub(crate) filters: Filters,
	/// The hashes of the filters held, to tell whether a reload changed the set.
	pub(crate) hashes: HashSet<[u8; 32]>,
	/// When upki last wrote its manifest, which it does at every successful fetch.
	pub(crate) confirmed: SystemTime,
}

/// Why one read failed.
enum Failure {
	/// A filter the index names was not there: an update may have been replacing the files.
	Missing,
	/// Anything else: the cache is not usable.
	Damaged(Error),
}

impl From<Error> for Failure {
	fn from(err: Error) -> Self {
		Self::Damaged(err)
	}
}

impl From<io::Error> for Failure {
	fn from(err: io::Error) -> Self {
		Self::Damaged(err.into())
	}
}

/// Read the cache in `dir`, the `revocation` directory of upki's cache.
///
/// A filter in `reuse` whose file is still there with the size the manifest gives, and whose hash
/// the manifest still lists, is not read or decoded again, since upki rewrites the manifest at
/// every fetch whether or not anything changed.
pub(crate) fn read(dir: &Path, reuse: &[Arc<Filter>]) -> Result<Snapshot, Error> {
	let reuse: HashMap<[u8; 32], &Arc<Filter>> =
		reuse.iter().map(|filter| (filter.hash, filter)).collect();

	let mut attempt = 0;
	loop {
		attempt += 1;
		match read_once(dir, &reuse) {
			Ok(snapshot) => return Ok(snapshot),
			Err(Failure::Missing) if attempt < ATTEMPTS => continue,
			Err(Failure::Missing) => {
				return Err(Error::Upki("a filter the index names is missing"));
			}
			Err(Failure::Damaged(err)) => return Err(err),
		}
	}
}

fn read_once(dir: &Path, reuse: &HashMap<[u8; 32], &Arc<Filter>>) -> Result<Snapshot, Failure> {
	// The modification time comes from the handle the manifest is read from, so it describes the
	// bytes read rather than whatever replaced them since.
	let manifest_file = File::open(dir.join("manifest.json"))?;
	let confirmed = manifest_file.metadata()?.modified()?;
	let manifest: Manifest =
		serde_json::from_slice(&read_capped(manifest_file, MAX_MANIFEST)?).map_err(Error::from)?;
	let files: HashMap<&str, &ManifestFile> = manifest
		.files
		.iter()
		.map(|file| (file.filename.as_str(), file))
		.collect();

	let names = index::filter_names(&read_capped(File::open(dir.join("index.bin"))?, MAX_INDEX)?)?;
	if names.is_empty() {
		return Err(Error::Upki("the index names no filters").into());
	}

	let mut filters = Vec::with_capacity(names.len());
	let mut hashes = HashSet::with_capacity(names.len());
	for name in &names {
		let Some(file) = files.get(name.as_str()) else {
			return Err(Error::Upki("a filter the index names is not in the manifest").into());
		};
		let hash: [u8; 32] = hex::decode(&file.hash)
			.ok()
			.and_then(|hash| hash.try_into().ok())
			.ok_or(Error::Upki("the manifest has a malformed hash"))?;

		let path = dir.join(name);
		let filter = match reuse.get(&hash) {
			Some(held) => {
				let meta = fs::metadata(&path).map_err(missing)?;
				if meta.len() != file.size {
					return Err(Error::FilterHash.into());
				}
				Arc::clone(held)
			}
			None => {
				let bytes = read_capped(File::open(&path).map_err(missing)?, MAX_FILE)?;
				if u64::try_from(bytes.len()) != Ok(file.size)
					|| <[u8; 32]>::from(Sha256::digest(&bytes)) != hash
				{
					return Err(Error::FilterHash.into());
				}
				Arc::new(Filter::decode(hash, &bytes)?)
			}
		};
		hashes.insert(hash);
		filters.push(filter);
	}

	Ok(Snapshot {
		filters: Filters { filters },
		hashes,
		confirmed,
	})
}

/// A file that is not there is a filter missing, which a reread may cure.
fn missing(err: io::Error) -> Failure {
	if err.kind() == io::ErrorKind::NotFound {
		Failure::Missing
	} else {
		err.into()
	}
}

/// Read `file` whole, refusing one longer than `cap` rather than reading it.
fn read_capped(file: File, cap: u64) -> Result<Vec<u8>, Error> {
	let mut bytes = Vec::new();
	file.take(cap + 1).read_to_end(&mut bytes)?;
	if u64::try_from(bytes.len()).is_ok_and(|len| len > cap) {
		return Err(Error::TooLarge);
	}
	Ok(bytes)
}

#[cfg(test)]
mod tests;
