use std::{
	fs,
	time::{Duration, SystemTime},
};

use super::*;
use crate::test_support::{
	crlite_filter, set_upki_manifest_time, write_upki_cache, write_upki_index, write_upki_manifest,
};
use crate::upki::Locator;

const FULL: &str = "20261017-1-default.filter";
const DELTA: &str = "20261018-0-default.filter.delta";

fn filter(serial: u8) -> Vec<u8> {
	crlite_filter(
		&[([serial; 32], vec![serial])],
		&[([serial; 32], vec![0xff])],
	)
}

fn confirmed() -> SystemTime {
	SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

struct Cache {
	dir: tempfile::TempDir,
	full: Vec<u8>,
	delta: Vec<u8>,
}

impl Cache {
	fn new() -> Self {
		let cache = Self {
			dir: tempfile::tempdir().expect("temp dir"),
			full: filter(1),
			delta: filter(2),
		};
		write_upki_cache(
			cache.dir.path(),
			&[(FULL, &cache.full), (DELTA, &cache.delta)],
			confirmed(),
		);
		cache
	}

	fn path(&self) -> &Path {
		self.dir.path()
	}

	fn read(&self) -> Result<Snapshot, Error> {
		read(self.path(), &[])
	}
}

fn damaged(result: Result<Snapshot, Error>) -> String {
	result.expect_err("an unusable cache").to_string()
}

#[test]
fn a_usable_cache_is_read() {
	let cache = Cache::new();
	let snapshot = cache.read().expect("usable");
	assert_eq!(snapshot.filters.filters.len(), 2);
	assert_eq!(snapshot.hashes.len(), 2);
	assert_eq!(snapshot.confirmed, confirmed());
}

#[test]
fn the_confirmed_time_is_the_manifests_modification_time() {
	let cache = Cache::new();
	let later = confirmed() + Duration::from_secs(3600);
	set_upki_manifest_time(cache.path(), later);
	assert_eq!(cache.read().expect("usable").confirmed, later);
}

#[test]
fn a_filter_missing_is_unusable_after_a_second_look() {
	let cache = Cache::new();
	fs::remove_file(cache.path().join(DELTA)).expect("removing");
	assert!(damaged(cache.read()).contains("missing"));
}

#[test]
fn a_filter_replaced_by_a_new_index_while_reading_is_found_on_the_second_look() {
	let cache = Cache::new();
	// The first attempt finds a name the old index still has; the second sees a settled cache.
	// Simulated by reading an index that names a file that is absent, then settling it.
	let extra = filter(3);
	write_upki_index(
		cache.path(),
		&[FULL, DELTA, "20261019-0-default.filter.delta"],
	);
	assert!(cache.read().is_err());

	write_upki_cache(
		cache.path(),
		&[
			(FULL, &cache.full),
			(DELTA, &cache.delta),
			("20261019-0-default.filter.delta", &extra),
		],
		confirmed(),
	);
	assert_eq!(cache.read().expect("settled").filters.filters.len(), 3);
}

#[test]
fn a_filter_with_the_wrong_hash_is_unusable() {
	let cache = Cache::new();
	fs::write(cache.path().join(DELTA), filter(9)).expect("replacing");
	assert!(damaged(cache.read()).contains("does not match"));
}

#[test]
fn a_filter_with_the_wrong_size_is_unusable() {
	let cache = Cache::new();
	let mut bytes = cache.delta.clone();
	bytes.push(0);
	fs::write(cache.path().join(DELTA), bytes).expect("replacing");
	assert!(cache.read().is_err());
}

#[test]
fn a_filter_the_manifest_does_not_list_is_unusable() {
	let cache = Cache::new();
	write_upki_manifest(cache.path(), &[(FULL, &cache.full)], confirmed());
	assert!(damaged(cache.read()).contains("not in the manifest"));
}

#[test]
fn a_cache_without_a_manifest_is_unusable() {
	let cache = Cache::new();
	fs::remove_file(cache.path().join("manifest.json")).expect("removing");
	assert!(cache.read().is_err());
}

#[test]
fn a_cache_without_an_index_is_unusable() {
	let cache = Cache::new();
	fs::remove_file(cache.path().join("index.bin")).expect("removing");
	assert!(cache.read().is_err());
}

#[test]
fn a_manifest_that_is_not_json_is_unusable() {
	let cache = Cache::new();
	fs::write(cache.path().join("manifest.json"), "not json").expect("replacing");
	assert!(cache.read().is_err());
}

#[test]
fn a_manifest_with_a_malformed_hash_is_unusable() {
	let cache = Cache::new();
	let manifest = format!(
		r#"{{"files":[{{"filename":"{FULL}","size":{},"hash":"zz"}},{{"filename":"{DELTA}","size":1,"hash":"00"}}]}}"#,
		cache.full.len(),
	);
	fs::write(cache.path().join("manifest.json"), manifest).expect("replacing");
	assert!(damaged(cache.read()).contains("malformed hash"));
}

#[test]
fn an_index_naming_nothing_is_unusable() {
	let cache = Cache::new();
	write_upki_index(cache.path(), &[]);
	assert!(damaged(cache.read()).contains("no filters"));
}

#[test]
fn a_filter_that_does_not_decode_is_unusable() {
	let cache = Cache::new();
	let junk = b"not a filter".to_vec();
	write_upki_cache(
		cache.path(),
		&[(FULL, &cache.full), (DELTA, &junk)],
		confirmed(),
	);
	assert!(cache.read().is_err());
}

#[test]
fn a_name_that_would_leave_the_directory_is_unusable() {
	let cache = Cache::new();
	let outside = cache.path().join("..").join("escape");
	fs::write(&outside, &cache.full).ok();
	write_upki_index(cache.path(), &["../escape"]);
	write_upki_manifest(cache.path(), &[("../escape", &cache.full)], confirmed());
	assert!(damaged(cache.read()).contains("plain file name"));
	let _ = fs::remove_file(outside);
}

#[test]
fn an_empty_directory_is_unusable() {
	let dir = tempfile::tempdir().expect("temp dir");
	assert!(read(dir.path(), &[]).is_err());
	assert!(read(&dir.path().join("absent"), &[]).is_err());
}

#[test]
fn a_filter_held_already_is_reused_rather_than_decoded_again() {
	let cache = Cache::new();
	let first = cache.read().expect("usable");
	let later = confirmed() + Duration::from_secs(21_600);
	set_upki_manifest_time(cache.path(), later);

	let second = read(cache.path(), &first.filters.filters).expect("usable");
	assert_eq!(second.confirmed, later);
	assert_eq!(second.hashes, first.hashes);
	for (new, old) in second.filters.filters.iter().zip(&first.filters.filters) {
		assert!(Arc::ptr_eq(new, old));
	}
}

#[test]
fn a_reused_filter_whose_file_is_gone_is_not_usable() {
	let cache = Cache::new();
	let first = cache.read().expect("usable");
	fs::remove_file(cache.path().join(FULL)).expect("removing");
	assert!(read(cache.path(), &first.filters.filters).is_err());
}

/// Reads whatever upki cache this host has, which a CI machine does not. Run with `--ignored`.
#[test]
#[ignore = "needs a upki cache on the host"]
fn the_hosts_own_cache_is_usable() {
	let dir = Locator::platform()
		.revocation_dir()
		.expect("upki's cache is located");
	let snapshot = read(&dir, &[]).expect("the host's cache is usable");
	assert!(!snapshot.filters.filters.is_empty());
}
