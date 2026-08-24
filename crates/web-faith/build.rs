use std::{env, fs, path::PathBuf};

fn main() {
	// The default user agent names the version of reqwest the request actually goes out on, which
	// only the lock file knows.
	let lock_path = find_cargo_lock().expect("Cargo.lock not found in any ancestor directory");
	let reqwest_version = extract_reqwest_version(&lock_path).unwrap();
	println!("cargo:rustc-env=REQWEST_VERSION={}", reqwest_version);
	println!("cargo:rerun-if-changed={}", lock_path.display());
}

/// The lock file lives at the workspace root, which is an ancestor of this crate.
fn find_cargo_lock() -> Option<PathBuf> {
	let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR")?);
	manifest_dir
		.ancestors()
		.map(|dir| dir.join("Cargo.lock"))
		.find(|lock| lock.is_file())
}

fn extract_reqwest_version(lock_path: &PathBuf) -> Option<String> {
	let cargo_lock = fs::read_to_string(lock_path).ok()?;

	// Find the reqwest package entry in Cargo.lock
	for line in cargo_lock.lines() {
		if line.starts_with("name = \"reqwest\"") {
			// Look for the version line in the next few lines
			let mut lines_iter = cargo_lock.lines().skip_while(|l| l != &line);
			lines_iter.next(); // Skip the name line

			for next_line in lines_iter.take(5) {
				if let Some(version) = next_line.trim().strip_prefix("version = \"") {
					if let Some(version) = version.strip_suffix("\"") {
						return Some(version.to_string());
					}
				}
			}
		}
	}

	None
}
