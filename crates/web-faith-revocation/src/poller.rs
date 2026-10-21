//! The poller: one thread per process, checking Remote Settings for changes.
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

use crate::{Source, remote_settings::RemoteSettings, store::Store};

/// The first retry after a failure; each further failure doubles it, up to the polling interval.
const FIRST_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// Start the poller thread, which checks the sources it is sent.
pub(crate) fn spawn(
	store: Arc<Store>,
	remote: RemoteSettings,
	interval: Duration,
) -> UnboundedSender<Source> {
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
			runtime.block_on(run(store, remote, interval, rx));
		});
	// Without a thread the data held stays as it is: nothing is fetched, nothing is refused that
	// was not already.
	drop(spawned);
	tx
}

async fn run(
	store: Arc<Store>,
	remote: RemoteSettings,
	interval: Duration,
	mut rx: UnboundedReceiver<Source>,
) {
	// The roots are listed here rather than at agent construction: on some platforms it is slow.
	store.rebuild_roots();

	let mut sources: Vec<Source> = Vec::new();
	let mut failures = 0u32;
	let mut due: Option<Instant> = None;

	loop {
		let wake = async {
			match due {
				Some(at) => tokio::time::sleep_until(at.into()).await,
				None => std::future::pending().await,
			}
		};

		tokio::select! {
			source = rx.recv() => {
				let Some(source) = source else { return };
				if !sources.contains(&source) {
					sources.push(source);
				}
				due = Some(next_due(&store, &sources, interval));
			}
			() = wake => {
				let ok = poll(&store, &remote, &sources).await;
				if ok {
					failures = 0;
					due = Some(next_due(&store, &sources, interval));
				} else {
					let backoff = FIRST_BACKOFF.saturating_mul(1 << failures.min(16)).min(interval);
					failures += 1;
					due = Some(Instant::now() + backoff);
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

/// Check every source once. `true` when each was confirmed current.
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
