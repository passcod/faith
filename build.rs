use std::fs;

fn main() {
	napi_build::setup();

	// Parse reqwest version from Cargo.lock
	let reqwest_version = extract_reqwest_version().unwrap();
	println!("cargo:rustc-env=REQWEST_VERSION={}", reqwest_version);
	println!("cargo:rerun-if-changed=Cargo.lock");
}

fn extract_reqwest_version() -> Option<String> {
	let cargo_lock = fs::read_to_string("Cargo.lock").ok()?;

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
