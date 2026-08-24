//! Reading a response: where trailers land, what is known of the peer, and writing a body out.

// spec:RESP spec:TRL spec:BODY

use std::{net::SocketAddr, time::Duration};

use http::header::HeaderMap;
use tokio::sync::watch;

use crate::error::{FaithError, FaithErrorKind};

/// What is known about the peer that sent a response.
///
/// - `address`: The IP address and port of the peer, if available.
/// - `certificate`: When connected over HTTPS, this is the DER-encoded leaf certificate of the peer.
#[derive(Debug)]
pub struct PeerInformation {
	pub address: Option<SocketAddr>,
	pub certificate: Option<Vec<u8>>,
}

/// Where a response body is written, and on what terms.
#[derive(Debug, Clone, Default)]
pub struct FileDestination {
	/// Truncate and replace an occupied destination. The default refuses one instead, leaving what
	/// is there untouched.
	pub overwrite: bool,
	/// The permissions a newly created file is given. Ignored on platforms without Unix file modes.
	pub mode: Option<u32>,
}

/// The shortest gap between progress reports.
///
/// Reporting every chunk would cross a surface boundary thousands of times for a large body,
/// which is the cost writing to a file directly exists to avoid. A caller driving a progress bar
/// cannot use updates faster than this anyway, and the final report is always delivered regardless.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// Open the destination file for a body write, mapping filesystem refusals to the errors
/// writing a body to a file surfaces.
// spec:BODY#tofile
pub async fn open_destination(
	path: &str,
	options: &FileDestination,
) -> Result<tokio::fs::File, FaithError> {
	let mut open = tokio::fs::OpenOptions::new();
	open.write(true);
	if options.overwrite {
		// An occupied destination is truncated and replaced.
		open.create(true).truncate(true);
	} else {
		// The safe default refuses an occupied destination outright.
		open.create_new(true);
	}
	#[cfg(unix)]
	if let Some(mode) = options.mode {
		open.mode(mode);
	}

	match open.open(path).await {
		Ok(file) => Ok(file),
		Err(err) => Err(classify_open_error(path, err).await),
	}
}

/// Classify a failure to open the destination. An occupied destination is `FileExists`,
/// unless what occupies it is a directory: a directory is well-formed but cannot be written
/// to, which is a `FileWrite`. Every other refusal is a `FileWrite` carrying the OS detail.
pub async fn classify_open_error(path: &str, err: std::io::Error) -> FaithError {
	let kind = if err.kind() == std::io::ErrorKind::AlreadyExists {
		match tokio::fs::symlink_metadata(path).await {
			Ok(meta) if meta.is_dir() => FaithErrorKind::FileWrite,
			_ => FaithErrorKind::FileExists,
		}
	} else {
		FaithErrorKind::FileWrite
	};
	FaithError::new(kind, Some(err.to_string()))
}

#[derive(Clone, Debug, Default)]
pub enum Trailers {
	#[default]
	NotYet,
	None,
	Some(HeaderMap),
}

/// Where the trailers land: written by whoever finishes the body, awaited by `trailers()`.
///
/// A watch channel, rather than a lock read in a loop. Per the fetch standard's trailers
/// proposal (<https://github.com/whatwg/fetch/pull/1940>) this promise is *meant* not to
/// resolve until the body has been consumed, so the wait is unbounded by design -- which is
/// precisely why polling was the wrong shape for it. Awaiting trailers without reading the
/// body now leaves an idle pending promise rather than a pegged core, and the future can be
/// cancelled while it waits.
#[derive(Debug)]
pub struct TrailersSlot(watch::Sender<Trailers>);

impl Default for TrailersSlot {
	fn default() -> Self {
		Self(watch::channel(Trailers::NotYet).0)
	}
}

impl TrailersSlot {
	/// Record trailers that arrived, waking whoever is waiting.
	pub fn arrived(&self, trailers: HeaderMap) {
		self.0.send_replace(Trailers::Some(trailers));
	}

	/// Record that the body ended, if no trailers frame got there first.
	///
	/// `send_if_modified` so the read and the write are one step, and so waiters are woken
	/// only by the call that actually settled it.
	pub fn ended(&self) {
		self.0.send_if_modified(|state| {
			if matches!(state, Trailers::NotYet) {
				*state = Trailers::None;
				true
			} else {
				false
			}
		});
	}

	/// Wait until the body has settled the question.
	pub async fn settled(&self) -> Trailers {
		let mut rx = self.0.subscribe();
		// `wait_for` tests the current value before waiting, so trailers that already
		// arrived return without yielding. Its error case is the sender being gone, which
		// means the response was dropped and nothing can ever set this -- no trailers is
		// the only answer left.
		match rx
			.wait_for(|state| !matches!(state, Trailers::NotYet))
			.await
		{
			Ok(state) => state.clone(),
			Err(_) => Trailers::None,
		}
	}
}

#[cfg(test)]
mod tests {
	use std::{
		future::Future,
		pin::pin,
		sync::atomic::{AtomicUsize, Ordering},
		task::{Context, Poll, Wake, Waker},
	};

	use super::*;

	/// A waker that counts how many times the task asks to be polled again.
	struct CountingWaker(AtomicUsize);

	impl CountingWaker {
		fn wakes(&self) -> usize {
			self.0.load(Ordering::SeqCst)
		}
	}

	impl Wake for CountingWaker {
		fn wake(self: Arc<Self>) {
			self.wake_by_ref();
		}

		fn wake_by_ref(self: &Arc<Self>) {
			self.0.fetch_add(1, Ordering::SeqCst);
		}
	}

	/// Waiting for trailers parks until the body settles the question, rather than polling
	/// for it.
	///
	/// The bug this guards against was a `yield_now` loop, which is visible here as the shape
	/// of the wait rather than as a quantity of CPU: a spin re-arms its own waker on every
	/// poll, so it is scheduled again immediately, while a parked wait asks for nothing until
	/// something else moves. Asserting the wake count keeps this deterministic -- timing how
	/// much CPU the process burns measures the machine as much as the code.
	#[test]
	fn waiting_for_trailers_parks_rather_than_spinning() {
		let slot = TrailersSlot::default();
		let counter = Arc::new(CountingWaker(AtomicUsize::new(0)));
		let waker = Waker::from(counter.clone());
		let mut cx = Context::from_waker(&waker);
		let mut settled = pin!(slot.settled());

		// Nothing has settled the question, so the wait parks...
		assert!(matches!(settled.as_mut().poll(&mut cx), Poll::Pending));
		// ...without scheduling itself to be polled again, which is what a spin does.
		assert_eq!(counter.wakes(), 0, "a parked wait asks for no wake-up");

		// Polling again changes nothing: still parked, still asking for nothing.
		assert!(matches!(settled.as_mut().poll(&mut cx), Poll::Pending));
		assert_eq!(counter.wakes(), 0, "polling again does not arm a wake-up");

		// The body ending is what wakes it, and it resolves on the next poll.
		slot.ended();
		assert!(counter.wakes() >= 1, "the body ending wakes the waiter");
		assert!(matches!(
			settled.as_mut().poll(&mut cx),
			Poll::Ready(Trailers::None)
		));
	}

	/// Trailers that arrived before anyone asked resolve without parking at all.
	#[test]
	fn trailers_already_there_resolve_on_the_first_poll() {
		let slot = TrailersSlot::default();
		let mut headers = HeaderMap::new();
		headers.insert("x-checksum", "abc123".parse().unwrap());
		slot.arrived(headers);

		let counter = Arc::new(CountingWaker(AtomicUsize::new(0)));
		let waker = Waker::from(counter.clone());
		let mut cx = Context::from_waker(&waker);
		let mut settled = pin!(slot.settled());

		assert!(matches!(
			settled.as_mut().poll(&mut cx),
			Poll::Ready(Trailers::Some(_))
		));
	}
}
