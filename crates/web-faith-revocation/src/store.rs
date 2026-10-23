//! The data each source holds, and how it changes.
//!
//! Every update goes through here, from the cache or the network alike, so the same checks apply
//! to both: the signature, the rollback guard, and for CRLite the filter hashes.

// spec:REVOC#keeping-current
// spec:REVOC#the-cache
// spec:REVOC#a-usable-cache
// spec:REVOC#a-fresh-cache
// spec:REVOC#choosing-between-providers

use std::{
	collections::HashMap,
	sync::{
		Arc, Mutex, Once, RwLock,
		atomic::{AtomicU64, Ordering},
	},
	time::SystemTime,
};

#[cfg(feature = "crlite")]
use std::collections::HashSet;
#[cfg(feature = "upki")]
use std::{path::PathBuf, time::Duration};

use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

#[cfg(feature = "crlite")]
use crate::crlite::{self, Channel, Filter, Filters};
#[cfg(feature = "onecrl")]
use crate::onecrl::{self, OneCrl};
#[cfg(feature = "upki")]
use crate::upki::{self, Locator};
use crate::{
	InForce, Provider, Source, Sources,
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
}

impl<T> Default for Held<T> {
	fn default() -> Self {
		Self {
			data: None,
			signed: None,
			verified_at: 0,
			confirmed: None,
			complete: false,
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

/// The CRLite data read from the system's upki cache, which Faith does not fetch, sign-check or
/// write, and so does not hold as a [`Held`].
#[cfg(feature = "upki")]
#[derive(Debug, Default)]
pub(crate) struct UpkiHeld {
	pub(crate) data: Option<Arc<Crlite>>,
	/// The hashes of the filters in `data`, to tell a reload that changed them from one that did
	/// not.
	hashes: HashSet<[u8; 32]>,
	/// When upki last fetched, as the data in `data` has it.
	pub(crate) confirmed: Option<SystemTime>,
	/// Whether `confirmed` is recent enough for the data to be the only CRLite data in force.
	/// Kept here, and updated by the poller's local check, so no handshake reads a clock.
	pub(crate) fresh: bool,
	/// The cache directory and manifest time of the last read, whether or not it worked, so a
	/// cache that stays unusable is not read again until it changes.
	seen: Option<(PathBuf, SystemTime)>,
}

/// How far ahead of the clock a manifest's modification time may be and still count as recent.
#[cfg(feature = "upki")]
const UPKI_CLOCK_SKEW: Duration = Duration::from_secs(60 * 60);

/// How the system's upki cache is found and judged.
#[cfg(feature = "upki")]
#[derive(Clone, Debug)]
pub(crate) struct UpkiSettings {
	pub(crate) location: upki::UpkiLocation,
	pub(crate) max_age: Duration,
}

/// Everything the process holds, shared by the verifiers and the poller.
#[derive(Debug)]
pub(crate) struct Store {
	pub(crate) trust: Trust,
	pub(crate) cache: Cache,
	/// Moves whenever any source's data changes, so saved sessions can be told stale.
	pub(crate) epoch: Arc<AtomicU64>,
	/// The epoch each server's certificate was last verified under, so a session ticket can be
	/// tagged with the data its connection was checked against rather than whatever is current
	/// when the ticket arrives.
	verified: Mutex<HashMap<ServerName<'static>, u64>>,
	#[cfg(feature = "onecrl")]
	pub(crate) onecrl: RwLock<Held<OneCrl>>,
	#[cfg(feature = "onecrl")]
	onecrl_load: Once,
	#[cfg(feature = "crlite")]
	pub(crate) crlite: RwLock<Held<Crlite>>,
	#[cfg(feature = "crlite")]
	crlite_load: Once,
	#[cfg(feature = "upki")]
	pub(crate) upki: RwLock<UpkiHeld>,
	#[cfg(feature = "upki")]
	upki_load: Once,
	#[cfg(feature = "upki")]
	upki_settings: UpkiSettings,
	pub(crate) roots: RwLock<Option<Arc<RootIndex>>>,
	roots_load: Once,
}

/// How many servers' verified epochs are remembered. One past it forgets an arbitrary server,
/// whose tickets then count as saved under no epoch and are never resumed.
const VERIFIED_CAPACITY: usize = 1024;

impl Store {
	pub(crate) fn new(
		trust: Trust,
		cache: Cache,
		#[cfg(feature = "upki")] upki_settings: UpkiSettings,
	) -> Self {
		Self {
			trust,
			cache,
			epoch: Arc::new(AtomicU64::new(0)),
			verified: Mutex::default(),
			#[cfg(feature = "onecrl")]
			onecrl: RwLock::default(),
			#[cfg(feature = "onecrl")]
			onecrl_load: Once::new(),
			#[cfg(feature = "crlite")]
			crlite: RwLock::default(),
			#[cfg(feature = "crlite")]
			crlite_load: Once::new(),
			#[cfg(feature = "upki")]
			upki: RwLock::default(),
			#[cfg(feature = "upki")]
			upki_load: Once::new(),
			#[cfg(feature = "upki")]
			upki_settings,
			roots: RwLock::default(),
			roots_load: Once::new(),
		}
	}

	fn bump_epoch(&self) {
		self.epoch.fetch_add(1, Ordering::AcqRel);
	}

	/// The epoch the data stands at now.
	pub(crate) fn current_epoch(&self) -> u64 {
		self.epoch.load(Ordering::Acquire)
	}

	/// Note that `server_name`'s certificate was verified against the data as of `epoch`, which
	/// the caller read before checking so that a change during the check counts against it.
	pub(crate) fn record_verified(&self, server_name: &ServerName<'_>, epoch: u64) {
		let mut verified = self.verified.lock().expect("poisoned");
		let name = server_name.to_owned();
		if verified.len() >= VERIFIED_CAPACITY
			&& !verified.contains_key(&name)
			&& let Some(victim) = verified.keys().next().cloned()
		{
			verified.remove(&victim);
		}
		verified.insert(name, epoch);
	}

	/// The epoch `server_name` was last verified under, or 0 when it is not known, which no
	/// session saved after the first update will pass for current.
	pub(crate) fn verified_epoch(&self, server_name: &ServerName<'_>) -> u64 {
		self.verified
			.lock()
			.expect("poisoned")
			.get(server_name)
			.copied()
			.unwrap_or(0)
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
			.then(|| self.crlite_choice(sources.uses_upki()))
			.flatten()
			.map(|(_, data, _)| data);

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

	/// Index the platform's roots if that has not been done yet. Slow on some platforms, so it runs
	/// while the first agent using a source is constructed, never on a handshake.
	pub(crate) fn load_roots(&self) {
		self.roots_load.call_once(|| self.rebuild_roots());
	}

	/// Rebuild the index of the platform's roots. Slow on some platforms, so never on a handshake.
	pub(crate) fn rebuild_roots(&self) {
		let index = Arc::new(RootIndex::platform());
		*self.roots.write().expect("poisoned") = Some(index);
	}

	/// The data in force for an agent using `sources`, and when it was last confirmed current.
	/// `None` until there is any.
	// spec:REVOC#reporting
	pub(crate) fn in_force(&self, source: Source, sources: Sources) -> Option<InForce> {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => {
				let held = self.onecrl.read().expect("poisoned");
				held.data.is_some().then_some(InForce {
					provider: Provider::RemoteSettings,
					confirmed: held.confirmed,
				})
			}
			#[cfg(feature = "crlite")]
			Source::Crlite => self
				.crlite_choice(sources.uses_upki())
				.map(|(provider, _, confirmed)| InForce {
					provider,
					confirmed,
				}),
		}
	}

	/// The CRLite data an agent is checked against, which provider it came from, and when it was
	/// last confirmed. An agent that uses upki is given the system's data while it is usable and
	/// fresh, and otherwise Faith's own unless the system's is stale but newer than it.
	// spec:REVOC#choosing-between-providers
	#[cfg(feature = "crlite")]
	#[cfg_attr(not(feature = "upki"), allow(unused_variables))]
	fn crlite_choice(&self, upki: bool) -> Option<(Provider, Arc<Crlite>, Option<SystemTime>)> {
		let own = || {
			let held = self.crlite.read().expect("poisoned");
			let data = held.data.clone()?;
			Some((Provider::RemoteSettings, data, held.confirmed))
		};

		#[cfg(feature = "upki")]
		if upki {
			let held = self.upki.read().expect("poisoned");
			if let Some(data) = held.data.clone() {
				let theirs = (Provider::Upki, data, held.confirmed);
				if held.fresh {
					return Some(theirs);
				}
				drop(held);
				// Stale data stays in force until Faith's own is newer, so there is no gap while
				// the first fetch lands.
				return match own() {
					Some(ours) if ours.2 > theirs.2 => Some(ours),
					_ => Some(theirs),
				};
			}
		}
		own()
	}

	fn crlite_provider(&self) -> Option<Provider> {
		#[cfg(feature = "crlite")]
		return self.crlite_choice(true).map(|(provider, ..)| provider);
		#[cfg(not(feature = "crlite"))]
		None
	}

	/// Whether the poller must keep Faith's own CRLite data current for an agent that uses upki
	/// as `upki` says: it does unless the system's data is usable and fresh.
	pub(crate) fn needs_own_crlite(&self, upki: bool) -> bool {
		!(upki && self.upki_fresh())
	}

	/// Whether the system's CRLite data is held and fresh.
	pub(crate) fn upki_fresh(&self) -> bool {
		#[cfg(feature = "upki")]
		return self.upki.read().expect("poisoned").fresh;
		#[cfg(not(feature = "upki"))]
		false
	}

	/// When Faith's own data for `source` was last confirmed current.
	pub(crate) fn confirmed(&self, source: Source) -> Option<SystemTime> {
		match source {
			#[cfg(feature = "onecrl")]
			Source::OneCrl => self.onecrl.read().expect("poisoned").confirmed,
			#[cfg(feature = "crlite")]
			Source::Crlite => self.crlite.read().expect("poisoned").confirmed,
		}
	}

	/// The held collection's timestamp, and whether everything it names is held. Faith's own data.
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
			// `Once` makes a second agent wait for the first one's read to finish.
			Source::OneCrl => self.onecrl_load.call_once(|| {
				// A cache that does not verify is as good as none.
				if let Some(cached) = self.cache.read_collection(onecrl::COLLECTION) {
					let _ = self.install_onecrl(
						cached.signed,
						cached.verified_at,
						cached.confirmed_at,
						false,
					);
				}
			}),
			#[cfg(feature = "crlite")]
			Source::Crlite => self.crlite_load.call_once(|| {
				// As for OneCRL, and a cache missing its full filter likewise.
				if let Some(cached) = self.cache.read_collection(crlite::COLLECTION) {
					let _ = self.install_crlite_from_disk(cached);
				}
			}),
		}
	}

	/// Record that the source's data was confirmed current at `now`, and write that to the cache.
	pub(crate) fn confirm(&self, source: Source, now: SystemTime) {
		// Faith's own data being newer can hand CRLite back from stale system data.
		let before = self.crlite_provider();
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
		if self.crlite_provider() != before {
			self.bump_epoch();
		}
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

	/// Read the system's upki cache for the first time, once per process. Synchronous, for the
	/// same reason as [`load`](Self::load).
	#[cfg(feature = "upki")]
	pub(crate) fn load_upki(&self) {
		self.upki_load.call_once(|| self.refresh_upki());
	}

	/// Check the system's upki cache without touching the network: read it again if its manifest
	/// has changed since the last read, replacing the data held only when the new data is usable,
	/// and work out whether the data held is still fresh.
	///
	/// The location is worked out afresh each time, so a cache that appears, moves or goes away
	/// is noticed. Data already held stays in force through a cache gone absent or unusable, and
	/// ages.
	// spec:REVOC#keeping-up-with-the-cache
	#[cfg(feature = "upki")]
	pub(crate) fn refresh_upki(&self) {
		let before = self.crlite_provider();
		let was_fresh = self.upki.read().expect("poisoned").fresh;
		let dir = match &self.upki_settings.location {
			upki::UpkiLocation::System => Locator::platform().revocation_dir(),
			upki::UpkiLocation::Dir(dir) => Some(dir.clone()),
			upki::UpkiLocation::Off => None,
		};

		let replaced = dir.is_some_and(|dir| self.reload_upki(dir));

		let mut held = self.upki.write().expect("poisoned");
		let fresh = self.upki_is_fresh(&held, SystemTime::now());
		let flipped = was_fresh != fresh;
		held.fresh = fresh;
		drop(held);

		if replaced || flipped || self.crlite_provider() != before {
			self.bump_epoch();
		}
	}

	/// Whether the data in `held` is fresh at `now`: held, and fetched by upki recently enough.
	///
	/// A manifest dated a little ahead of `now` is a clock that was slightly off. One dated further
	/// ahead than that says nothing about when upki last fetched, so it is not fresh.
	#[cfg(feature = "upki")]
	fn upki_is_fresh(&self, held: &UpkiHeld, now: SystemTime) -> bool {
		held.data.is_some()
			&& held
				.confirmed
				.is_some_and(|at| match now.duration_since(at) {
					Ok(age) => age <= self.upki_settings.max_age,
					Err(ahead) => ahead.duration() <= UPKI_CLOCK_SKEW,
				})
	}

	/// Read the cache in `dir` if its manifest has changed since it was last read. `true` when
	/// that replaced the data held.
	#[cfg(feature = "upki")]
	fn reload_upki(&self, dir: PathBuf) -> bool {
		let stat = std::fs::metadata(dir.join("manifest.json"))
			.and_then(|meta| meta.modified())
			.ok();

		let reuse: Vec<Arc<Filter>> = {
			let held = self.upki.read().expect("poisoned");
			if let Some(stat) = stat
				&& held.seen.as_ref() == Some(&(dir.clone(), stat))
			{
				return false;
			}
			held.data
				.iter()
				.flat_map(|data| data.filters.filters.iter().cloned())
				.collect()
		};
		let Some(stat) = stat else {
			// Nothing to read; whatever turns up next is read.
			self.upki.write().expect("poisoned").seen = None;
			return false;
		};

		// Not under the lock: a first read decodes megabytes.
		let read = upki::read(&dir, &reuse);
		let mut held = self.upki.write().expect("poisoned");
		match read {
			Ok(snapshot) => {
				held.seen = Some((dir, snapshot.confirmed));
				held.confirmed = Some(snapshot.confirmed);
				if held.data.is_some() && held.hashes == snapshot.hashes {
					held.fresh = self.upki_is_fresh(&held, SystemTime::now());
					return false;
				}
				held.data = Some(Arc::new(Crlite {
					filters: snapshot.filters,
				}));
				held.hashes = snapshot.hashes;
				// Judged here, under the lock that swaps the data, so no handshake sees the new
				// data with the old data's verdict.
				held.fresh = self.upki_is_fresh(&held, SystemTime::now());
				true
			}
			// An unusable cache leaves the data held in force.
			Err(_) => {
				held.seen = Some((dir, stat));
				false
			}
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
