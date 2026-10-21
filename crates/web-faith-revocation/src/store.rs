//! The data each source holds, and how it changes.
//!
//! Every update goes through here, from the cache or the network alike, so the same checks apply
//! to both: the signature, the rollback guard, and for CRLite the filter hashes.

// spec:REVOC#keeping-current
// spec:REVOC#the-cache

use std::{
	sync::{
		Arc, RwLock,
		atomic::{AtomicU64, Ordering},
	},
	time::SystemTime,
};

#[cfg(feature = "crlite")]
use std::collections::{HashMap, HashSet};

use rustls_pki_types::{CertificateDer, UnixTime};

#[cfg(feature = "crlite")]
use crate::crlite::{self, Channel, Filter, Filters};
#[cfg(feature = "onecrl")]
use crate::onecrl::{self, OneCrl};
use crate::{
	Source, Sources,
	cache::{Cache, Cached, from_secs, secs},
	error::Error,
	links::{Links, RootIndex},
	remote_settings::{RemoteSettings, Signed},
	signature::Trust,
};

/// One source's data and what is known about it.
#[derive(Debug)]
pub(crate) struct Held<T> {
	pub(crate) data: Option<Arc<T>>,
	/// The changeset the data came from, kept to rewrite the cache when it is confirmed.
	signed: Option<Arc<Signed>>,
	verified_at: u64,
	pub(crate) confirmed: Option<SystemTime>,
	/// Whether everything the signed records name is held. Only CRLite can fall short, by an
	/// incremental filter that failed to download, in which case the poller tries again.
	pub(crate) complete: bool,
	/// Whether the cache has been read for this source.
	loaded: bool,
}

impl<T> Default for Held<T> {
	fn default() -> Self {
		Self {
			data: None,
			signed: None,
			verified_at: 0,
			confirmed: None,
			complete: false,
			loaded: false,
		}
	}
}

impl<T> Held<T> {
	pub(crate) fn timestamp(&self) -> Option<u64> {
		self.signed.as_ref().map(|s| s.changeset.timestamp)
	}
}

/// CRLite's data: the channel's records and the filters obtained for them.
#[cfg(feature = "crlite")]
#[derive(Debug)]
pub(crate) struct Crlite {
	pub(crate) filters: Filters,
}

/// Everything the process holds, shared by the verifiers and the poller.
#[derive(Debug)]
pub(crate) struct Store {
	pub(crate) trust: Trust,
	pub(crate) cache: Cache,
	/// Moves whenever any source's data changes, so saved sessions can be told stale.
	pub(crate) epoch: Arc<AtomicU64>,
	#[cfg(feature = "onecrl")]
	pub(crate) onecrl: RwLock<Held<OneCrl>>,
	#[cfg(feature = "crlite")]
	pub(crate) crlite: RwLock<Held<Crlite>>,
	pub(crate) roots: RwLock<Option<Arc<RootIndex>>>,
}

impl Store {
	pub(crate) fn new(trust: Trust, cache: Cache) -> Self {
		Self {
			trust,
			cache,
			epoch: Arc::new(AtomicU64::new(0)),
			#[cfg(feature = "onecrl")]
			onecrl: RwLock::default(),
			#[cfg(feature = "crlite")]
			crlite: RwLock::default(),
			roots: RwLock::default(),
		}
	}

	fn bump_epoch(&self) {
		self.epoch.fetch_add(1, Ordering::AcqRel);
	}

	/// The source that refuses this chain, if any. OneCRL is asked first.
	// spec:REVOC#where-the-check-sits
	pub(crate) fn refusing_source(
		&self,
		end_entity: &CertificateDer<'_>,
		intermediates: &[CertificateDer<'_>],
		sources: Sources,
		extra_roots: Option<&RootIndex>,
	) -> Option<Source> {
		#[cfg(feature = "onecrl")]
		let onecrl = sources
			.onecrl
			.then(|| self.onecrl.read().expect("poisoned").data.clone())
			.flatten();
		#[cfg(feature = "crlite")]
		let crlite = sources
			.crlite
			.then(|| self.crlite.read().expect("poisoned").data.clone())
			.flatten();

		// Linking costs a parse of every certificate, so it is skipped when no source has data.
		let mut any = false;
		#[cfg(feature = "onecrl")]
		{
			any |= onecrl.is_some();
		}
		#[cfg(feature = "crlite")]
		{
			any |= crlite.is_some();
		}
		if !any {
			return None;
		}

		let roots = self.roots.read().expect("poisoned").clone();
		let links = Links::new(end_entity, intermediates, roots.as_deref(), extra_roots)?;

		#[cfg(feature = "onecrl")]
		if onecrl.is_some_and(|set| set.refuses(&links)) {
			return Some(Source::OneCrl);
		}
		#[cfg(feature = "crlite")]
		if crlite.is_some_and(|data| data.filters.refuses(&links)) {
			return Some(Source::Crlite);
		}

		None
	}

	/// Rebuild the index of the platform's roots. Slow on some platforms, so never on a handshake.
	pub(crate) fn rebuild_roots(&self) {
		let index = Arc::new(RootIndex::platform());
		*self.roots.write().expect("poisoned") = Some(index);
	}

	pub(crate) fn has_data(&self, source: Source) -> bool {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => self.onecrl.read().expect("poisoned").data.is_some(),
			#[cfg(feature = "crlite")]
			Source::Crlite => self.crlite.read().expect("poisoned").data.is_some(),
		}
	}

	pub(crate) fn confirmed(&self, source: Source) -> Option<SystemTime> {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => self.onecrl.read().expect("poisoned").confirmed,
			#[cfg(feature = "crlite")]
			Source::Crlite => self.crlite.read().expect("poisoned").confirmed,
		}
	}

	/// The held collection's timestamp, and whether everything it names is held.
	pub(crate) fn held(&self, source: Source) -> (Option<u64>, bool) {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => {
				let held = self.onecrl.read().expect("poisoned");
				(held.timestamp(), held.complete)
			}
			#[cfg(feature = "crlite")]
			Source::Crlite => {
				let held = self.crlite.read().expect("poisoned");
				(held.timestamp(), held.complete)
			}
		}
	}

	/// Read a source's cached data, once per process. Synchronous: it runs while the first agent
	/// using the source is constructed, so no handshake waits on the disk.
	pub(crate) fn load(&self, source: Source) {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => {
				let mut held = self.onecrl.write().expect("poisoned");
				if std::mem::replace(&mut held.loaded, true) {
					return;
				}
				drop(held);
				// A cache that does not verify is as good as none.
				if let Some(cached) = self.cache.read_collection(onecrl::COLLECTION) {
					let _ = self.install_onecrl(
						cached.signed,
						cached.verified_at,
						cached.confirmed_at,
						false,
					);
				}
			}
			#[cfg(feature = "crlite")]
			Source::Crlite => {
				let mut held = self.crlite.write().expect("poisoned");
				if std::mem::replace(&mut held.loaded, true) {
					return;
				}
				drop(held);
				// As for OneCRL, and a cache missing its full filter likewise.
				if let Some(cached) = self.cache.read_collection(crlite::COLLECTION) {
					let _ = self.install_crlite_from_disk(cached);
				}
			}
		}
	}

	/// Record that the source's data was confirmed current at `now`, and write that to the cache.
	pub(crate) fn confirm(&self, source: Source, now: SystemTime) {
		let (signed, verified_at, collection) = match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => {
				let mut held = self.onecrl.write().expect("poisoned");
				held.confirmed = Some(now);
				(held.signed.clone(), held.verified_at, onecrl::COLLECTION)
			}
			#[cfg(feature = "crlite")]
			Source::Crlite => {
				let mut held = self.crlite.write().expect("poisoned");
				held.confirmed = Some(now);
				(held.signed.clone(), held.verified_at, crlite::COLLECTION)
			}
		};
		if let Some(signed) = signed {
			self.cache.write_collection(
				collection,
				&Cached {
					signed: Signed::clone(&signed),
					verified_at,
					confirmed_at: secs(now),
				},
			);
		}
	}

	/// Verify a OneCRL changeset and make it the held data. `fresh` marks one straight from the
	/// network, which is written to the cache.
	#[cfg(feature = "onecrl")]
	pub(crate) fn install_onecrl(
		&self,
		signed: Signed,
		verified_at: u64,
		confirmed_at: u64,
		fresh: bool,
	) -> Result<(), Error> {
		let held_timestamp = self.onecrl.read().expect("poisoned").timestamp();
		signed.verify(&self.trust, unix_time(verified_at), held_timestamp)?;
		let set = OneCrl::from_records(&signed.records())?;

		if fresh {
			self.cache.write_collection(
				onecrl::COLLECTION,
				&Cached {
					signed: signed.clone(),
					verified_at,
					confirmed_at,
				},
			);
		}

		let mut held = self.onecrl.write().expect("poisoned");
		held.data = Some(Arc::new(set));
		held.signed = Some(Arc::new(signed));
		held.verified_at = verified_at;
		held.confirmed = Some(from_secs(confirmed_at));
		held.complete = true;
		drop(held);
		self.bump_epoch();
		Ok(())
	}

	/// Verify a cached CRLite changeset and load the filters it names from the cache.
	#[cfg(feature = "crlite")]
	fn install_crlite_from_disk(&self, cached: Cached) -> Result<(), Error> {
		let held_timestamp = self.crlite.read().expect("poisoned").timestamp();
		cached
			.signed
			.verify(&self.trust, unix_time(cached.verified_at), held_timestamp)?;
		let channel = Channel::from_records(&cached.signed.records())?;

		let mut filters = Vec::new();
		let mut complete = true;
		for record in channel.all() {
			let hash = record
				.attachment
				.sha256()
				.expect("checked when the channel was read");
			let filter = self
				.cache
				.read_filter(&hash)
				.filter(|bytes| record.attachment.matches(bytes))
				.and_then(|bytes| Filter::decode(hash, &bytes).ok());
			match filter {
				Some(filter) => filters.push(Arc::new(filter)),
				// Without the full filter there is nothing to anchor the incremental ones to.
				None if !record.incremental => return Err(Error::FilterHash),
				None => complete = false,
			}
		}

		self.set_crlite(
			cached.signed,
			cached.verified_at,
			cached.confirmed_at,
			filters,
			complete,
		);
		Ok(())
	}

	/// Verify a CRLite changeset from the network, obtain the filters it names, and make them the
	/// held data. Filters already held, or already in the cache, are not downloaded again. The
	/// held data is replaced only once the full filter is obtained.
	#[cfg(feature = "crlite")]
	pub(crate) async fn install_crlite(
		&self,
		remote: &RemoteSettings,
		signed: Signed,
		now: SystemTime,
	) -> Result<(), Error> {
		let held_timestamp = self.crlite.read().expect("poisoned").timestamp();
		signed.verify(&self.trust, unix_time(secs(now)), held_timestamp)?;
		let channel = Channel::from_records(&signed.records())?;

		let held: HashMap<[u8; 32], Arc<Filter>> = self
			.crlite
			.read()
			.expect("poisoned")
			.data
			.iter()
			.flat_map(|data| data.filters.filters.iter())
			.map(|filter| (filter.hash, Arc::clone(filter)))
			.collect();

		let mut base = None;
		let mut filters = Vec::new();
		let mut complete = true;
		for record in channel.all() {
			let hash = record
				.attachment
				.sha256()
				.expect("checked when the channel was read");
			let filter = match held.get(&hash) {
				Some(filter) => Ok(Arc::clone(filter)),
				None => self.obtain_filter(remote, &mut base, record, hash).await,
			};
			match filter {
				Ok(filter) => filters.push(filter),
				Err(err) if !record.incremental => return Err(err),
				Err(_) => complete = false,
			}
		}

		let named: HashSet<[u8; 32]> = channel
			.all()
			.filter_map(|record| record.attachment.sha256())
			.collect();
		let at = secs(now);
		self.cache.write_collection(
			crlite::COLLECTION,
			&Cached {
				signed: signed.clone(),
				verified_at: at,
				confirmed_at: at,
			},
		);
		self.set_crlite(signed, at, at, filters, complete);
		self.cache.prune_filters(&named, now);
		Ok(())
	}

	#[cfg(feature = "crlite")]
	async fn obtain_filter(
		&self,
		remote: &RemoteSettings,
		base: &mut Option<reqwest::Url>,
		record: &crlite::FilterRecord,
		hash: [u8; 32],
	) -> Result<Arc<Filter>, Error> {
		if let Some(bytes) = self.cache.read_filter(&hash)
			&& record.attachment.matches(&bytes)
		{
			return Ok(Arc::new(Filter::decode(hash, &bytes)?));
		}

		let base = match base {
			Some(base) => base,
			None => base.insert(remote.attachments_base().await?),
		};
		let bytes = remote
			.attachment(base, &record.attachment.location, record.attachment.size)
			.await?;
		if !record.attachment.matches(&bytes) {
			return Err(Error::FilterHash);
		}
		let filter = Filter::decode(hash, &bytes)?;
		self.cache.write_filter(&hash, &bytes);
		Ok(Arc::new(filter))
	}

	#[cfg(feature = "crlite")]
	fn set_crlite(
		&self,
		signed: Signed,
		verified_at: u64,
		confirmed_at: u64,
		filters: Vec<Arc<Filter>>,
		complete: bool,
	) {
		let mut held = self.crlite.write().expect("poisoned");
		held.data = Some(Arc::new(Crlite {
			filters: Filters { filters },
		}));
		held.signed = Some(Arc::new(signed));
		held.verified_at = verified_at;
		held.confirmed = Some(from_secs(confirmed_at));
		held.complete = complete;
		drop(held);
		self.bump_epoch();
	}
}

fn unix_time(secs: u64) -> UnixTime {
	UnixTime::since_unix_epoch(std::time::Duration::from_secs(secs))
}
