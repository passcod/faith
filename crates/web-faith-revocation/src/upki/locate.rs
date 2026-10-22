//! Finding upki's cache directory, as upki does.

// spec:REVOC#locating-the-cache

use std::{
	fs, io,
	path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::Deserialize;

/// Where Faith reads upki's CRLite data from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum UpkiLocation {
	/// Find the cache as upki does: its configuration file's `cache-dir`, or the default location
	/// for the platform. The lookup is redone at each check, so a cache that appears or moves
	/// while the process runs is found.
	#[default]
	System,
	/// The `revocation` directory itself, which holds `index.bin` and `manifest.json`.
	Dir(PathBuf),
	/// Do not look for a cache at all.
	Off,
}

/// The directories upki looks in, as values rather than read from the environment each time, so a
/// test can name its own.
#[derive(Clone, Debug)]
pub(crate) struct Locator {
	pub(crate) user_config_dir: Option<PathBuf>,
	pub(crate) user_cache_dir: Option<PathBuf>,
	pub(crate) system_config_dir: PathBuf,
	pub(crate) system_cache_dir: PathBuf,
}

/// The one setting of upki's configuration file that Faith needs.
#[derive(Deserialize)]
struct Config {
	#[serde(rename = "cache-dir")]
	cache_dir: PathBuf,
}

impl Locator {
	/// The directories upki uses on this platform, read from the environment now.
	pub(crate) fn platform() -> Self {
		let dirs = ProjectDirs::from("dev", "rustls", "upki");
		let (system_config_dir, system_cache_dir) = system_dirs();
		Self {
			user_config_dir: dirs.as_ref().map(|dirs| dirs.config_dir().to_owned()),
			user_cache_dir: dirs.as_ref().map(|dirs| dirs.cache_dir().to_owned()),
			system_config_dir,
			system_cache_dir,
		}
	}

	/// The directory holding `index.bin` and `manifest.json`, if upki's cache can be located.
	///
	/// The user's configuration file wins over the system one. A file that exists but cannot be
	/// read or parsed, or has no `cache-dir`, leaves the cache unlocated rather than falling back
	/// to a default upki itself would not have used. Without any configuration file the cache is
	/// upki's per-user cache directory if there is one, and otherwise the system's.
	pub(crate) fn revocation_dir(&self) -> Option<PathBuf> {
		let candidates = self
			.user_config_dir
			.iter()
			.chain([&self.system_config_dir])
			.map(|dir| dir.join("config.toml"));
		for path in candidates {
			match fs::read_to_string(&path) {
				Ok(text) => {
					let config: Config = toml::from_str(&text).ok()?;
					return Some(config.cache_dir.join("revocation"));
				}
				Err(err) if absent(&err) => continue,
				Err(_) => return None,
			}
		}

		self.user_cache_dir
			.iter()
			.chain([&self.system_cache_dir])
			.find(|dir| Path::new(dir).is_dir())
			.map(|dir| dir.join("revocation"))
	}
}

/// Whether `err` means the path is not there, as opposed to being there and unusable.
fn absent(err: &io::Error) -> bool {
	matches!(
		err.kind(),
		io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
	)
}

#[cfg(windows)]
fn system_dirs() -> (PathBuf, PathBuf) {
	let program_data = std::env::var_os("ProgramData")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
	let root = program_data.join("rustls").join("upki");
	(root.clone(), root.join("cache"))
}

#[cfg(target_os = "macos")]
fn system_dirs() -> (PathBuf, PathBuf) {
	(
		PathBuf::from("/Library/Application Support/upki"),
		PathBuf::from("/Library/Caches/dev.rustls.upki"),
	)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_dirs() -> (PathBuf, PathBuf) {
	(PathBuf::from("/etc/upki"), PathBuf::from("/var/cache/upki"))
}

#[cfg(test)]
mod tests;
