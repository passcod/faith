use std::fs;

use serde_json::json;

use super::*;
use crate::remote_settings::{Changeset, Metadata, Signature};

fn cached(timestamp: u64) -> Cached {
	Cached {
		signed: Signed {
			changeset: Changeset {
				metadata: Metadata {
					signature: Signature {
						x5u: "https://example.com/chain".into(),
						signature: "sig".into(),
					},
				},
				timestamp,
				changes: vec![json!({ "id": "a" })],
			},
			chain: "chain".into(),
		},
		verified_at: 10,
		confirmed_at: 20,
	}
}

#[test]
fn a_collection_round_trips() {
	let dir = tempfile::tempdir().expect("temp dir");
	let cache = Cache::new(Some(dir.path().join("nested")));
	assert!(cache.read_collection("onecrl").is_none());

	cache.write_collection("onecrl", &cached(5));
	let read = cache.read_collection("onecrl").expect("written");
	assert_eq!(read.signed.changeset.timestamp, 5);
	assert_eq!(read.confirmed_at, 20);

	// Nothing but the file itself is left behind.
	let names: Vec<_> = fs::read_dir(dir.path().join("nested"))
		.expect("dir")
		.map(|e| e.expect("entry").file_name())
		.collect();
	assert_eq!(names, vec!["onecrl.json"]);
}

#[test]
fn a_filter_round_trips() {
	let dir = tempfile::tempdir().expect("temp dir");
	let cache = Cache::new(Some(dir.path().to_owned()));
	let hash = [7u8; 32];
	assert!(cache.read_filter(&hash).is_none());
	cache.write_filter(&hash, b"bytes");
	assert_eq!(cache.read_filter(&hash).as_deref(), Some(&b"bytes"[..]));
}

#[test]
fn without_a_directory_nothing_is_kept() {
	let cache = Cache::new(None);
	cache.write_collection("onecrl", &cached(5));
	cache.write_filter(&[1; 32], b"bytes");
	assert!(cache.read_collection("onecrl").is_none());
	assert!(cache.read_filter(&[1; 32]).is_none());
	cache.prune_filters(&HashSet::new(), SystemTime::now());
}

#[test]
fn an_unwritable_directory_is_not_an_error() {
	let dir = tempfile::tempdir().expect("temp dir");
	let file = dir.path().join("a-file");
	fs::write(&file, b"not a directory").expect("write");
	let cache = Cache::new(Some(file));
	cache.write_collection("onecrl", &cached(5));
	assert!(cache.read_collection("onecrl").is_none());
}

#[test]
fn a_filter_no_record_names_goes_after_the_grace_period() {
	let dir = tempfile::tempdir().expect("temp dir");
	let cache = Cache::new(Some(dir.path().to_owned()));
	let (kept, dropped) = ([1u8; 32], [2u8; 32]);
	cache.write_filter(&kept, b"kept");
	cache.write_filter(&dropped, b"dropped");
	let named = HashSet::from([kept]);

	let now = SystemTime::now();
	cache.prune_filters(&named, now);
	assert!(
		cache.read_filter(&dropped).is_some(),
		"kept through the grace period"
	);

	cache.prune_filters(&named, now + PRUNE_GRACE / 2);
	assert!(cache.read_filter(&dropped).is_some());

	// The clock starts when it first went unnamed, not at each prune.
	cache.prune_filters(&named, now + PRUNE_GRACE + Duration::from_secs(1));
	assert!(cache.read_filter(&dropped).is_none());
	assert!(cache.read_filter(&kept).is_some());
}

#[test]
fn a_filter_named_again_is_not_pruned() {
	let dir = tempfile::tempdir().expect("temp dir");
	let cache = Cache::new(Some(dir.path().to_owned()));
	let hash = [3u8; 32];
	cache.write_filter(&hash, b"bytes");

	let now = SystemTime::now();
	cache.prune_filters(&HashSet::new(), now);
	cache.prune_filters(&HashSet::from([hash]), now + PRUNE_GRACE / 2);
	cache.prune_filters(&HashSet::new(), now + PRUNE_GRACE + Duration::from_secs(1));
	assert!(cache.read_filter(&hash).is_some(), "its clock restarted");
}
