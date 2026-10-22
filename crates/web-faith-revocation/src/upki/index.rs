//! The filter names in upki's `index.bin`.
//!
//! The index also carries a directory of Certificate Transparency logs and the window of
//! timestamps each filter covers, so that upki can load only the filter a certificate needs.
//! Faith loads every filter the index names and asks each, and a filter answers "not covered" for
//! a timestamp outside its own window, so only the table of names is read.

// spec:REVOC#a-usable-cache

use crate::error::Error;

const MAGIC_V1: &[u8; 8] = b"upkiidx1";
const MAGIC_V0: &[u8; 8] = b"upkiidx0";

/// A name is a 32-byte slot, padded with NULs.
const NAME_SLOT: usize = 32;

/// A log directory entry: the log's ID, the offset of its entries, and how many there are.
const LOG_SLOT: usize = 32 + 8 + 2;

/// The largest `index.bin` read. Real ones are tens of kilobytes.
pub(crate) const MAX_INDEX: u64 = 16 * 1024 * 1024;

/// The filter names `index`, the contents of an `index.bin`, lists, in order and without repeats.
///
/// Every length in the header is checked against the file's, and every name is checked to be a
/// plain file name, so that none can leave the cache directory when it is joined onto it.
pub(crate) fn filter_names(index: &[u8]) -> Result<Vec<String>, Error> {
	let bad = |why| Err(Error::Upki(why));

	let Some((magic, rest)) = index.split_first_chunk::<8>() else {
		return bad("index.bin is too short");
	};
	// Version 0 counts the names in one byte rather than two.
	let (count, rest) = if magic == MAGIC_V1 {
		let Some((count, rest)) = rest.split_first_chunk::<2>() else {
			return bad("index.bin is too short");
		};
		(usize::from(u16::from_be_bytes(*count)), rest)
	} else if magic == MAGIC_V0 {
		let Some((&count, rest)) = rest.split_first() else {
			return bad("index.bin is too short");
		};
		(usize::from(count), rest)
	} else {
		return bad("index.bin is not a recognised version");
	};
	let Some((logs, rest)) = rest.split_first_chunk::<4>() else {
		return bad("index.bin is too short");
	};
	let logs = u32::from_be_bytes(*logs);

	// Neither table may run past the file, however large the header claims it to be.
	let names_len = count * NAME_SLOT;
	let Some(logs_len) = usize::try_from(logs)
		.ok()
		.and_then(|logs| logs.checked_mul(LOG_SLOT))
	else {
		return bad("index.bin's log table overflows");
	};
	let Some(tables_len) = names_len.checked_add(logs_len) else {
		return bad("index.bin's tables overflow");
	};
	if tables_len > rest.len() {
		return bad("index.bin is shorter than its header says");
	}

	let mut names: Vec<String> = Vec::with_capacity(count);
	for slot in rest[..names_len].chunks_exact(NAME_SLOT) {
		let end = slot.iter().position(|&byte| byte == 0).unwrap_or(NAME_SLOT);
		let Ok(name) = std::str::from_utf8(&slot[..end]) else {
			return bad("index.bin names a filter that is not text");
		};
		if !is_plain_file_name(name) {
			return bad("index.bin names a filter that is not a plain file name");
		}
		if !names.iter().any(|seen| seen == name) {
			names.push(name.to_owned());
		}
	}
	Ok(names)
}

/// Whether `name` is one upki's cache can hold: 1 to 32 of letters, digits, `-`, `.` and `_`, and
/// neither `.` nor `..`. Anything else could name a path outside the cache directory.
pub(crate) fn is_plain_file_name(name: &str) -> bool {
	(1..=NAME_SLOT).contains(&name.len())
		&& name != "."
		&& name != ".."
		&& name
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
}

#[cfg(test)]
mod tests;
