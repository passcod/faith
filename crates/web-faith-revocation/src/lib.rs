//! Certificate revocation checking for rustls, from Mozilla's OneCRL and CRLite.
//!
//! [`RevocationVerifier`] wraps another [`ServerCertVerifier`], such as the platform verifier, and
//! refuses a chain that verifier accepted when a revocation source lists a certificate on it. It
//! only ever adds refusals.
//!
//! - **OneCRL** lists the intermediate and root certificates Mozilla has revoked.
//! - **CRLite** covers leaf certificates from the publicly trusted authorities enrolled in it, as
//!   compact filters queried with the leaf's Certificate Transparency timestamps.
//!
//! Both are fetched from Mozilla Remote Settings, checked against their content signature, kept
//! current by a background poller, and cached on disk so a restart enforces the last known data
//! from its first connection.
//!
//! A [`Revocation`] holds that data. Make one per process and share it: every verifier built from
//! it reads the same data, and the poller fetches once for all of them.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use rustls::ClientConfig;
//! use web_faith_revocation::{Refusals, Revocation, RevocationConfig, Source, Sources};
//!
//! let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
//! let mut config = RevocationConfig::mozilla(Arc::clone(&provider));
//! config.cache_dir = Some("/var/cache/my-app/revocation".into());
//!
//! let revocation = Revocation::new(config, reqwest::Client::new());
//! revocation.enable(Source::OneCrl);
//! revocation.enable(Source::Crlite);
//!
//! let platform = Arc::new(rustls_platform_verifier::Verifier::new(Arc::clone(&provider))?);
//! let verifier = revocation.verifier(platform, Sources::all(), &[], Arc::new(Refusals::default()));
//!
//! let mut tls = ClientConfig::builder_with_provider(provider)
//!     .with_safe_default_protocol_versions()?
//!     .dangerous()
//!     .with_custom_certificate_verifier(verifier)
//!     .with_no_client_auth();
//! tls.resumption = rustls::client::Resumption::store(revocation.session_store());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Features
//!
//! | Feature | Default | What it adds |
//! | --- | :-: | --- |
//! | `onecrl` | ✓ | The OneCRL source. |
//! | `crlite` | ✓ | The CRLite source, and its MPL-2.0 filter decoder `clubcard-crlite`. |
//! | `test-support` |  | A fake Remote Settings server and signing chain, for tests. |

// spec:REVOC

#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
// With a source compiled out, the machinery only it reaches goes unused.
#![cfg_attr(
	not(all(feature = "onecrl", feature = "crlite")),
	allow(
		dead_code,
		unused_imports,
		unused_mut,
		unused_variables,
		unreachable_code
	)
)]

use std::{
	fmt,
	path::PathBuf,
	sync::{Arc, Mutex, atomic::AtomicU64},
	time::{Duration, SystemTime},
};

#[cfg(any(feature = "onecrl", feature = "crlite"))]
use std::sync::atomic::Ordering;

use reqwest::Url;
use rustls::{
	client::{ClientSessionStore, danger::ServerCertVerifier},
	crypto::CryptoProvider,
};
use rustls_pki_types::{CertificateDer, ServerName};
use tokio::sync::mpsc::UnboundedSender;

mod cache;
mod cert;
#[cfg(feature = "crlite")]
mod crlite;
mod error;
mod links;
#[cfg(feature = "onecrl")]
mod onecrl;
mod poller;
mod remote_settings;
mod session;
mod signature;
mod store;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;
mod verifier;

pub use error::{Error, Revoked};
pub use session::EpochSessionStore;
pub use verifier::RevocationVerifier;

use crate::{
	cache::Cache, links::RootIndex, remote_settings::RemoteSettings, signature::Trust, store::Store,
};

/// Mozilla's production Remote Settings server.
pub const MOZILLA_REMOTE_SETTINGS: &str = "https://firefox.settings.services.mozilla.com/v1/";

/// The SHA-256 of Mozilla's content-signing root certificate, as Firefox pins it.
pub const MOZILLA_ROOT_HASH: [u8; 32] = [
	0xc8, 0xa8, 0x0e, 0x9a, 0xfa, 0xef, 0x4e, 0x21, 0x9b, 0x6f, 0xb5, 0xd7, 0xa7, 0x1d, 0x0f, 0x10,
	0x12, 0x23, 0xba, 0xc5, 0x00, 0x1a, 0xc2, 0x8f, 0x9b, 0x0d, 0x43, 0xdc, 0x59, 0xa1, 0x06, 0xdb,
];

/// The name the signing certificate for both collections carries.
pub const MOZILLA_SIGNER: &str = "onecrl.content-signature.mozilla.org";

/// How often the poller checks for changes, as Firefox does.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// A revocation source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source {
	/// Mozilla's list of revoked intermediate and root certificates.
	#[cfg(feature = "onecrl")]
	OneCrl,
	/// Mozilla's filters of revoked leaf certificates.
	#[cfg(feature = "crlite")]
	Crlite,
}

impl Source {
	fn collection(self) -> &'static str {
		match self {
			#[cfg(feature = "onecrl")]
			Self::OneCrl => onecrl::COLLECTION,
			#[cfg(feature = "crlite")]
			Self::Crlite => crlite::COLLECTION,
		}
	}
}

impl fmt::Display for Source {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match *self {
			#[cfg(feature = "onecrl")]
			Self::OneCrl => f.write_str("OneCRL"),
			#[cfg(feature = "crlite")]
			Self::Crlite => f.write_str("CRLite"),
		}
	}
}

/// Which sources a verifier checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sources {
	/// Check OneCRL.
	#[cfg(feature = "onecrl")]
	pub onecrl: bool,
	/// Check CRLite.
	#[cfg(feature = "crlite")]
	pub crlite: bool,
}

impl Sources {
	/// Every source compiled in.
	pub const fn all() -> Self {
		Self {
			#[cfg(feature = "onecrl")]
			onecrl: true,
			#[cfg(feature = "crlite")]
			crlite: true,
		}
	}

	/// No source.
	pub const fn none() -> Self {
		Self {
			#[cfg(feature = "onecrl")]
			onecrl: false,
			#[cfg(feature = "crlite")]
			crlite: false,
		}
	}

	/// The sources turned on.
	pub fn iter(self) -> impl Iterator<Item = Source> {
		let mut on = Vec::new();
		#[cfg(feature = "onecrl")]
		if self.onecrl {
			on.push(Source::OneCrl);
		}
		#[cfg(feature = "crlite")]
		if self.crlite {
			on.push(Source::Crlite);
		}
		on.into_iter()
	}
}

impl Default for Sources {
	fn default() -> Self {
		Self::all()
	}
}

/// How many connections each source has refused, for one set of verifiers.
///
/// Pass the same one to every verifier whose refusals should be counted together.
#[derive(Debug, Default)]
pub struct Refusals {
	#[cfg(feature = "onecrl")]
	onecrl: AtomicU64,
	#[cfg(feature = "crlite")]
	crlite: AtomicU64,
}

impl Refusals {
	/// The number of connections `source` has refused.
	pub fn count(&self, source: Source) -> u64 {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => self.onecrl.load(Ordering::Relaxed),
			#[cfg(feature = "crlite")]
			Source::Crlite => self.crlite.load(Ordering::Relaxed),
		}
	}

	fn add(&self, source: Source) {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => self.onecrl.fetch_add(1, Ordering::Relaxed),
			#[cfg(feature = "crlite")]
			Source::Crlite => self.crlite.fetch_add(1, Ordering::Relaxed),
		};
	}
}

/// Where revocation data comes from, how it is checked, and where it is cached.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct RevocationConfig {
	/// The Remote Settings server's API root, ending in `/`.
	pub base_url: Url,
	/// The SHA-256 of the content-signing root certificate the signing chains must end at.
	pub root_hash: [u8; 32],
	/// The name the signing certificate must carry.
	pub signer: ServerName<'static>,
	/// Where to cache data between runs. `None` holds it in memory only.
	pub cache_dir: Option<PathBuf>,
	/// How often to check for changes.
	pub poll_interval: Duration,
	/// The crypto provider that checks signatures.
	pub crypto_provider: Arc<CryptoProvider>,
}

impl RevocationConfig {
	/// Mozilla's production data, uncached.
	pub fn mozilla(crypto_provider: Arc<CryptoProvider>) -> Self {
		Self {
			base_url: Url::parse(MOZILLA_REMOTE_SETTINGS).expect("a valid URL"),
			root_hash: MOZILLA_ROOT_HASH,
			signer: ServerName::try_from(MOZILLA_SIGNER).expect("a valid DNS name"),
			cache_dir: None,
			poll_interval: DEFAULT_POLL_INTERVAL,
			crypto_provider,
		}
	}
}

/// The revocation data a process holds, and the poller keeping it current.
///
/// Cheap to clone: clones share the same data.
#[derive(Clone, Debug)]
pub struct Revocation {
	inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
	store: Arc<Store>,
	remote: RemoteSettings,
	poll_interval: Duration,
	poller: Mutex<Option<UnboundedSender<Source>>>,
}

impl Revocation {
	/// Hold revocation data as `config` describes, fetching it with `client`.
	///
	/// Nothing is read or fetched until a source is [enabled](Self::enable).
	pub fn new(config: RevocationConfig, client: reqwest::Client) -> Self {
		let mut base_url = config.base_url;
		if !base_url.path().ends_with('/') {
			base_url.set_path(&format!("{}/", base_url.path()));
		}

		let trust = Trust {
			root_hash: config.root_hash,
			signer: config.signer,
			provider: config.crypto_provider,
		};
		Self {
			inner: Arc::new(Inner {
				store: Arc::new(Store::new(trust, Cache::new(config.cache_dir))),
				remote: RemoteSettings::new(client, base_url),
				poll_interval: config.poll_interval,
				poller: Mutex::new(None),
			}),
		}
	}

	/// Start enforcing `source`: read its cached data now, synchronously, and have the poller keep
	/// it current, starting the poller if it is not running yet.
	///
	/// Until the source has data, from the cache or the network, it refuses nothing.
	// spec:REVOC#the-cache
	pub fn enable(&self, source: Source) {
		self.inner.store.load(source);

		let mut poller = self.inner.poller.lock().expect("poisoned");
		let sender = poller.get_or_insert_with(|| {
			poller::spawn(
				Arc::clone(&self.inner.store),
				self.inner.remote.clone(),
				self.inner.poll_interval,
			)
		});
		// A poller whose thread could not start has no receiver; the cached data stays in force.
		let _ = sender.send(source);
	}

	/// A verifier that runs `inner`, then checks `sources`.
	///
	/// `extra_roots` are roots `inner` trusts beyond the platform's, so paths through them are
	/// linked too. `refusals` counts the connections this verifier refuses.
	pub fn verifier(
		&self,
		inner: Arc<dyn ServerCertVerifier>,
		sources: Sources,
		extra_roots: &[CertificateDer<'_>],
		refusals: Arc<Refusals>,
	) -> Arc<RevocationVerifier> {
		Arc::new(RevocationVerifier {
			inner,
			store: Arc::clone(&self.inner.store),
			sources,
			extra_roots: (!extra_roots.is_empty())
				.then(|| Arc::new(RootIndex::new(extra_roots.iter()))),
			refusals,
		})
	}

	/// A session store that does not resume a session saved before the data last changed, since a
	/// resumed session skips the verifier. Use it on every config the verifier is on.
	// spec:REVOC#where-the-check-sits
	pub fn session_store(&self) -> Arc<dyn ClientSessionStore> {
		Arc::new(EpochSessionStore::new(Arc::clone(&self.inner.store.epoch)))
	}

	/// When `source`'s data was last confirmed current, or `None` before it has any.
	// spec:REVOC#reporting
	pub fn last_confirmed(&self, source: Source) -> Option<SystemTime> {
		self.inner
			.store
			.has_data(source)
			.then(|| self.inner.store.confirmed(source))
			.flatten()
	}
}
