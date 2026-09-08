//! What an HTTP cache is installed from, held so a rebuilt client keeps the same store.

use http_cache_reqwest::{CACacheManager, CacheMode, HttpCacheOptions, MokaManager};

/// The HTTP cache store to install on a client, held as the built manager rather than as the
/// options that produced it.
///
/// The manager *is* the store: `MokaManager` holds the cached entries behind an `Arc`, and
/// `CACacheManager` names the directory holding them. So cloning one shares the cache, while
/// building a fresh one from the same options would empty an in-memory cache — which is why a
/// client rebuilt for a network change clones this.
// spec:NETCHG#what-the-signal-keeps
#[derive(Debug, Clone)]
pub(crate) enum HttpCacheStore {
	Disk(CACacheManager),
	Memory(MokaManager),
}

/// The HTTP cache middleware to install.
#[derive(Debug, Clone)]
pub(crate) struct HttpCacheRecipe {
	pub(crate) mode: CacheMode,
	pub(crate) options: HttpCacheOptions,
	pub(crate) store: HttpCacheStore,
}
