//! The poller: one thread per process, checking Remote Settings for changes and the system's upki
//! cache for a refresh.
//!
//! It runs on a thread of its own with its own single-threaded runtime rather than on a caller's
//! runtime, which could stop under it, and the thread is detached, so it neither keeps a Rust
//! `main` from returning nor holds a Node event loop open.

// spec:REVOC#keeping-current

use std::{
	sync::Arc,
	time::{Duration, Instant, SystemTime},
};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::{Source, Sources, remote_settings::RemoteSettings, store::Store};

/// The first retry after a failure; each further failure doubles it, up to the polling interval.
const FIRST_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// How often the poller checks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Intervals {
	/// For Remote Settings.
	pub(crate) remote: Duration,
	/// For the system's upki cache, which is a `stat` unless it changed.
	#[cfg(feature = "upki")]
	pub(crate) upki: Duration,
}

/// What the agents the poller has heard of want, which decides what it fetches.
#[derive(Debug, Default)]
struct Demand {
	followed: Vec<Source>,
	/// Some agent uses CRLite and takes it from the system's cache where it can.
	crlite_with_upki: bool,
	/// Some agent uses CRLite and takes it from Faith's own fetch only.
	crlite_without_upki: bool,
}

impl Demand {
	/// Note an agent's sources. `true` when that changed anything.
	fn follow(&mut self, sources: Sources) -> bool {
		let mut changed = false;
		for source in sources.iter() {
			if !self.followed.contains(&source) {
				self.followed.push(source);
				changed = true;
			}
			#[cfg(feature = "crlite")]
			if source == Source::Crlite {
				let flag = if sources.uses_upki() {
					&mut self.crlite_with_upki
				} else {
					&mut self.crlite_without_upki
				};
				changed |= !std::mem::replace(flag, true);
			}
		}
		changed
	}

	/// Whether Faith's own CRLite data has to be kept current: an agent does not use the system's,
	/// or the system's is not usable and fresh.
	fn wants_own_crlite(&self, store: &Store) -> bool {
		self.crlite_without_upki || (self.crlite_with_upki && store.needs_own_crlite(true))
	}

	/// Whether the system's cache needs watching.
	#[cfg(feature = "upki")]
	fn watches_upki(&self) -> bool {
		self.crlite_with_upki
	}

	/// The sources to check at Remote Settings now. CRLite is left out while the system supplies it.
	fn to_fetch(&self, store: &Store) -> Vec<Source> {
		let own = self.wants_own_crlite(store);
		self.followed
			.iter()
			.copied()
			.filter(|&source| {
				#[cfg(feature = "crlite")]
				if source == Source::Crlite {
					return own;
				}
				true
			})
			.collect()
	}
}

/// Start the poller thread, which checks the sources it is told agents use.
pub(crate) fn spawn(
	store: Arc<Store>,
	remote: RemoteSettings,
	intervals: Intervals,
) -> UnboundedSender<Sources> {
	let (tx, rx) = unbounded_channel();
	let spawned = std::thread::Builder::new()
		.name("web-faith-revocation".into())
		.spawn(move || {
			let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
			else {
				return;
			};
			runtime.block_on(run(store, remote, intervals, rx));
		});
	// Without a thread the data held stays as it is: nothing is fetched, nothing is refused that
	// was not already.
	drop(spawned);
	tx
}

async fn sleep_until_some(at: Option<Instant>) {
	match at {
		Some(at) => tokio::time::sleep_until(at.into()).await,
		None => std::future::pending().await,
	}
}

async fn run(
	store: Arc<Store>,
	remote: RemoteSettings,
	intervals: Intervals,
	mut rx: UnboundedReceiver<Sources>,
) {
	let mut demand = Demand::default();
	let mut failures = 0u32;
	let mut due: Option<Instant> = None;
	// When the system's cache is next looked at. It is read locally, so a cache upki has just
	// refreshed is noticed within minutes rather than at the next Remote Settings check.
	let mut upki_due: Option<Instant> = None;

	loop {
		tokio::select! {
			sources = rx.recv() => {
				let Some(sources) = sources else { return };
				// Sources already followed change nothing, and while checks are failing the
				// backoff runs its course rather than starting over for each new agent.
				if demand.follow(sources) {
					#[cfg(feature = "upki")]
					if demand.watches_upki() && upki_due.is_none() {
						upki_due = Some(Instant::now() + intervals.upki);
					}
					if demand.wants_own_crlite(&store) {
						// Normally done already, while the agent was constructed.
						#[cfg(feature = "crlite")]
						store.load(Source::Crlite);
					}
					if failures == 0 {
						let active = demand.to_fetch(&store);
						due = (!active.is_empty())
							.then(|| next_due(&store, &active, intervals.remote));
					}
				}
			}
			() = sleep_until_some(due) => {
				let active = demand.to_fetch(&store);
				if active.is_empty() {
					// Nothing is wanted from Remote Settings, as when the system's cache supplies
					// all there is. The next change of demand sets the timer again.
					failures = 0;
					due = None;
				} else if poll(&store, &remote, &active).await {
					failures = 0;
					due = Some(next_due(&store, &active, intervals.remote));
				} else {
					let backoff = FIRST_BACKOFF
						.saturating_mul(1 << failures.min(16))
						.min(intervals.remote);
					failures += 1;
					due = Some(Instant::now() + backoff);
				}
			}
			() = sleep_until_some(upki_due) => {
				#[cfg(feature = "upki")]
				{
					let wanted = demand.wants_own_crlite(&store);
					store.refresh_upki();
					upki_due = Some(Instant::now() + intervals.upki);
					if !wanted && demand.wants_own_crlite(&store) {
						// The system's cache went stale or unusable: Faith's own data is wanted
						// now rather than at the next daily check.
						#[cfg(feature = "crlite")]
						store.load(Source::Crlite);
						due = Some(Instant::now());
					}
				}
				#[cfg(not(feature = "upki"))]
				{
					upki_due = None;
				}
			}
		}
	}
}

/// When the next check is due: at once for a source with no data or with filters still missing,
/// otherwise once the least recently confirmed source is an interval old. A host whose cache was
/// confirmed recently costs no request when an agent starts.
fn next_due(store: &Store, sources: &[Source], interval: Duration) -> Instant {
	let now = Instant::now();
	let mut due = now + interval;
	for &source in sources {
		let (_, complete) = store.held(source);
		let age = store
			.confirmed(source)
			.and_then(|at| SystemTime::now().duration_since(at).ok());
		let at = match age {
			Some(age) if complete => now + interval.saturating_sub(age),
			_ => now,
		};
		due = due.min(at);
	}
	due
}

/// Check each of `sources` once. `true` when each was confirmed current.
async fn poll(store: &Store, remote: &RemoteSettings, sources: &[Source]) -> bool {
	let collections: Vec<&str> = sources.iter().map(|source| source.collection()).collect();
	let Ok(modified) = remote.last_modified(&collections).await else {
		return false;
	};

	let mut ok = true;
	for (&source, modified) in sources.iter().zip(modified) {
		let Some(modified) = modified else {
			ok = false;
			continue;
		};
		let (held, complete) = store.held(source);
		let now = SystemTime::now();
		let current = held == Some(modified) && complete;
		if current || update(store, remote, source, modified, now).await {
			store.confirm(source, now);
			#[cfg(feature = "onecrl")]
			if source == Source::OneCrl && !current {
				store.rebuild_roots();
			}
			// An incremental filter that failed to download is retried on the backoff schedule.
			if !store.held(source).1 {
				ok = false;
			}
		} else {
			ok = false;
		}
	}
	ok
}

async fn update(
	store: &Store,
	remote: &RemoteSettings,
	source: Source,
	modified: u64,
	now: SystemTime,
) -> bool {
	let Ok(signed) = remote.changeset(source.collection(), modified).await else {
		return false;
	};
	match source {
		#[cfg(feature = "onecrl")]
		Source::OneCrl => {
			let at = crate::cache::secs(now);
			store.install_onecrl(signed, at, at, true).is_ok()
		}
		#[cfg(feature = "crlite")]
		Source::Crlite => store.install_crlite(remote, signed, now).await.is_ok(),
	}
}
