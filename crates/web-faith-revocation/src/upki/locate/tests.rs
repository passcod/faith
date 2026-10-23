use std::fs;

use super::*;

struct Dirs {
	_root: tempfile::TempDir,
	locator: Locator,
}

impl Dirs {
	fn new() -> Self {
		let root = tempfile::tempdir().expect("temp dir");
		let locator = Locator {
			user_config_dir: Some(root.path().join("user-config")),
			user_cache_dir: Some(root.path().join("user-cache")),
			system_config_dir: root.path().join("system-config"),
			system_cache_dir: root.path().join("system-cache"),
		};
		Self {
			_root: root,
			locator,
		}
	}

	fn user_config(&self, text: &str) {
		write(self.locator.user_config_dir.as_ref().expect("set"), text);
	}

	fn system_config(&self, text: &str) {
		write(&self.locator.system_config_dir, text);
	}
}

fn write(dir: &Path, text: &str) {
	fs::create_dir_all(dir).expect("config dir");
	fs::write(dir.join("config.toml"), text).expect("config file");
}

fn mkdir(dir: &Path) {
	fs::create_dir_all(dir).expect("dir");
}

#[test]
fn the_users_config_wins_over_the_systems() {
	let dirs = Dirs::new();
	dirs.user_config("cache-dir = '/user'");
	dirs.system_config("cache-dir = '/system'");
	assert_eq!(
		dirs.locator.revocation_dir(),
		Some(PathBuf::from("/user/revocation"))
	);
}

#[test]
fn the_system_config_is_used_without_a_users() {
	let dirs = Dirs::new();
	dirs.system_config("cache-dir = '/system'");
	assert_eq!(
		dirs.locator.revocation_dir(),
		Some(PathBuf::from("/system/revocation"))
	);
}

#[test]
fn a_config_names_the_cache_even_where_no_directory_exists() {
	let dirs = Dirs::new();
	mkdir(&dirs.locator.system_cache_dir);
	dirs.system_config("cache-dir = '/elsewhere'");
	assert_eq!(
		dirs.locator.revocation_dir(),
		Some(PathBuf::from("/elsewhere/revocation"))
	);
}

#[test]
fn unknown_keys_and_tables_are_ignored() {
	let dirs = Dirs::new();
	dirs.system_config(
		"cache-dir = '/system'\nsomething-new = 1\n\n[revocation]\nfetch-url = 'https://example.com/'\n",
	);
	assert_eq!(
		dirs.locator.revocation_dir(),
		Some(PathBuf::from("/system/revocation"))
	);
}

#[test]
fn without_a_config_the_user_cache_is_preferred() {
	let dirs = Dirs::new();
	mkdir(&dirs.locator.user_cache_dir.clone().expect("set"));
	mkdir(&dirs.locator.system_cache_dir);
	assert_eq!(
		dirs.locator.revocation_dir(),
		dirs.locator
			.user_cache_dir
			.map(|dir| dir.join("revocation"))
	);
}

#[test]
fn without_a_config_or_a_user_cache_the_system_one_is_used() {
	let dirs = Dirs::new();
	mkdir(&dirs.locator.system_cache_dir);
	assert_eq!(
		dirs.locator.revocation_dir(),
		Some(dirs.locator.system_cache_dir.join("revocation"))
	);
}

#[test]
fn nothing_present_is_unlocated() {
	assert_eq!(Dirs::new().locator.revocation_dir(), None);
}

#[test]
fn a_malformed_config_leaves_the_cache_unlocated() {
	let dirs = Dirs::new();
	mkdir(&dirs.locator.system_cache_dir);
	dirs.system_config("cache-dir = [");
	assert_eq!(dirs.locator.revocation_dir(), None);
}

#[test]
fn a_config_without_a_cache_dir_leaves_the_cache_unlocated() {
	let dirs = Dirs::new();
	mkdir(&dirs.locator.system_cache_dir);
	dirs.system_config("[revocation]\nfetch-url = 'https://example.com/'\n");
	assert_eq!(dirs.locator.revocation_dir(), None);
}

#[test]
fn a_relative_cache_dir_leaves_the_cache_unlocated() {
	let dirs = Dirs::new();
	mkdir(&dirs.locator.system_cache_dir);
	dirs.system_config("cache-dir = 'relative/cache'");
	assert_eq!(dirs.locator.revocation_dir(), None);
}

#[test]
fn a_broken_user_config_does_not_fall_through_to_the_systems() {
	let dirs = Dirs::new();
	dirs.user_config("not toml at all =");
	dirs.system_config("cache-dir = '/system'");
	assert_eq!(dirs.locator.revocation_dir(), None);
}

#[test]
fn a_config_that_cannot_be_read_leaves_the_cache_unlocated() {
	let dirs = Dirs::new();
	// A directory where the file should be is present but cannot be read as one.
	mkdir(&dirs.locator.system_config_dir.join("config.toml"));
	assert_eq!(dirs.locator.revocation_dir(), None);
}

#[test]
fn the_platform_locator_resolves_without_panicking() {
	let _ = Locator::platform().revocation_dir();
}
