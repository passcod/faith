use super::*;

/// An index with a version 1 header, `names` in the table, and `logs` empty log slots.
fn index_v1(names: &[&[u8]], logs: u32) -> Vec<u8> {
	let mut bytes = b"upkiidx1".to_vec();
	bytes.extend(u16::try_from(names.len()).expect("few names").to_be_bytes());
	bytes.extend(logs.to_be_bytes());
	for name in names {
		let mut slot = [0u8; NAME_SLOT];
		slot[..name.len()].copy_from_slice(name);
		bytes.extend(slot);
	}
	bytes.resize(bytes.len() + logs as usize * LOG_SLOT, 0);
	bytes
}

fn index_v0(names: &[&[u8]], logs: u32) -> Vec<u8> {
	let mut bytes = b"upkiidx0".to_vec();
	bytes.push(u8::try_from(names.len()).expect("few names"));
	bytes.extend(logs.to_be_bytes());
	for name in names {
		let mut slot = [0u8; NAME_SLOT];
		slot[..name.len()].copy_from_slice(name);
		bytes.extend(slot);
	}
	bytes.resize(bytes.len() + logs as usize * LOG_SLOT, 0);
	bytes
}

fn names(bytes: &[u8]) -> Vec<String> {
	filter_names(bytes).expect("a valid index")
}

#[test]
fn a_version_1_index_names_its_filters_in_order() {
	let index = index_v1(&[b"1-default.filter", b"2-default.filter.delta"], 3);
	assert_eq!(names(&index), ["1-default.filter", "2-default.filter.delta"]);
}

#[test]
fn a_version_0_index_names_its_filters_too() {
	let index = index_v0(&[b"a.filter", b"b.delta"], 2);
	assert_eq!(names(&index), ["a.filter", "b.delta"]);
}

#[test]
fn a_name_filling_its_whole_slot_is_read() {
	let name = [b'a'; 32];
	assert_eq!(names(&index_v1(&[&name], 0)), ["a".repeat(32)]);
}

#[test]
fn a_repeated_name_is_listed_once() {
	assert_eq!(names(&index_v1(&[b"a", b"b", b"a"], 0)), ["a", "b"]);
}

#[test]
fn an_index_naming_nothing_names_nothing() {
	assert!(names(&index_v1(&[], 4)).is_empty());
}

#[test]
fn the_wrong_magic_is_refused() {
	let mut index = index_v1(&[b"a"], 0);
	index[7] = b'2';
	assert!(filter_names(&index).is_err());
	assert!(filter_names(b"").is_err());
}

#[test]
fn a_truncated_header_is_refused() {
	let index = index_v1(&[b"a"], 0);
	for len in [0, 7, 8, 9, 13] {
		assert!(filter_names(&index[..len]).is_err(), "{len}");
	}
	let index = index_v0(&[b"a"], 0);
	for len in [8, 12] {
		assert!(filter_names(&index[..len]).is_err(), "{len}");
	}
}

#[test]
fn a_name_count_beyond_the_file_is_refused() {
	let mut index = index_v1(&[b"a"], 0);
	index[8..10].copy_from_slice(&u16::MAX.to_be_bytes());
	assert!(filter_names(&index).is_err());
}

#[test]
fn a_log_count_beyond_the_file_is_refused() {
	let mut index = index_v1(&[b"a"], 0);
	index[10..14].copy_from_slice(&1000u32.to_be_bytes());
	assert!(filter_names(&index).is_err());
}

#[test]
fn a_log_count_that_overflows_is_refused() {
	let mut index = index_v1(&[b"a"], 0);
	index[10..14].copy_from_slice(&u32::MAX.to_be_bytes());
	assert!(filter_names(&index).is_err());
}

#[test]
fn a_table_ending_mid_name_is_refused() {
	let index = index_v1(&[b"a", b"b"], 0);
	assert!(filter_names(&index[..index.len() - 5]).is_err());
}

#[test]
fn names_that_could_leave_the_directory_are_refused() {
	for bad in [
		&b".."[..],
		b".",
		b"../x",
		b"a/b",
		b"a\\b",
		b"/etc/passwd",
		b"c:x",
		b"with space",
		&[0xff, 0xfe],
	] {
		// An empty name, from an unused slot, is refused too.
		let index = index_v1(&[bad], 0);
		assert!(filter_names(&index).is_err(), "{:?}", String::from_utf8_lossy(bad));
	}
	assert!(filter_names(&index_v1(&[b""], 0)).is_err());
}

#[test]
fn a_nul_inside_a_name_ends_it() {
	// Bytes after the first NUL are padding whatever they hold, so this reads as "ab".
	let index = index_v1(&[b"ab\0cd"], 0);
	assert_eq!(names(&index), ["ab"]);
}

#[test]
fn the_name_rules() {
	for good in ["a", "A-z_0.9", "20261017-1-default.filter", &"x".repeat(32)] {
		assert!(is_plain_file_name(good), "{good}");
	}
	for bad in ["", ".", "..", "a/b", "a\\b", "a b", "é", &"x".repeat(33)] {
		assert!(!is_plain_file_name(bad), "{bad}");
	}
}
