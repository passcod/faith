//! Reading a response: where trailers land, what is known of the peer, and writing a body out.

// spec:RESP spec:TRL spec:BODY

use std::{
	hint::unreachable_unchecked,
	mem::replace,
	net::SocketAddr,
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, Instant},
};

use bytes::Bytes;
use futures::{StreamExt, stream};
use http::header::{CONTENT_LENGTH, HeaderMap};
use http_body_util::BodyStream;
use reqwest::{StatusCode, Url, Version};
use stream_shared::SharedStream;
use tokio::{io::AsyncWriteExt, sync::watch};
use web_faith_encoding::{Coding, decode_stream};

use crate::{
	body::{Body, BodyHolder, DynStream},
	error::{FaithError, FaithErrorKind},
	stats::InnerAgentStats,
	timing::TimingSlot,
};

use crate::integrity::{finish_integrity, integrity_checker, verify_integrity};

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

/// A progress report from a body write in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileProgress {
	/// Bytes written to the file so far.
	pub bytes_written: u64,
	/// What the response advertised in `Content-Length`, when it sent one and the body is not
	/// being decoded. Absent when the total is not known ahead of time, which is the case for a
	/// chunked response and for one being decoded.
	pub content_length: Option<u64>,
}

/// What a completed body write reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWritten {
	/// The absolute filesystem path written to.
	pub path: String,
	/// The number of bytes that landed at the destination.
	pub bytes_written: u64,
}

/// A response to a request.
///
/// A response is not constructed by a caller; it arrives from a request. Reading its body consumes
/// it, following the fetch standard rather than the owned-response model of other Rust clients, so a
/// second read fails.
#[derive(Debug, Clone)]
pub struct Response {
	pub body: BodyHolder,
	/// The coding to decode the body under, or `None` to deliver it as received.
	/// Set once when the response is built, from the request's `Accept-Encoding` and the
	/// response's `Content-Encoding` (see [`web_faith_encoding`]).
	pub decode: Option<Coding>,
	pub disturbed: Arc<AtomicBool>,
	pub headers: HeaderMap,
	pub integrity: Option<String>,
	pub peer: Arc<PeerInformation>,
	pub redirected: bool,
	pub stats: Arc<InnerAgentStats>,
	pub status_code: StatusCode,
	pub timing: Arc<TimingSlot>,
	pub trailers: Arc<TrailersSlot>,
	pub url: Url,
	pub version: Version,
}

impl Response {
	pub fn check_stream_disturbed(&self) -> Result<(), FaithError> {
		if self.disturbed.swap(true, Ordering::SeqCst) {
			Err(FaithErrorKind::ResponseAlreadyDisturbed.into())
		} else {
			Ok(())
		}
	}

	/// Ensures the body is converted to a SharedStream, returning a clone of it.
	///
	/// This allows multiple consumers (original + clones) to independently read the body.
	pub fn ensure_stream(
		&self,
		body: &mut Body,
		drained_flag: Arc<AtomicBool>,
	) -> Result<SharedStream<Pin<Box<DynStream>>>, FaithError> {
		match body {
			Body::Consumed => Err(FaithErrorKind::ResponseAlreadyDisturbed.into()),
			Body::Stream(stream) => Ok(stream.clone()),
			lock @ Body::Inner(_) => {
				// temporarily replace with Consumed until we can put in the Stream
				let Body::Inner(inner) = replace(lock, Body::Consumed) else {
					// SAFETY: we're inside the match checking for this exact thing
					unsafe { unreachable_unchecked() }
				};

				// Track that we've started consuming a body
				self.stats.bodies_started.fetch_add(1, Ordering::Relaxed);

				let trailers_stream = self.trailers.clone();
				let trailers_finish = self.trailers.clone();
				let stats_finish = self.stats.clone();
				let timing_finish = self.timing.clone();
				let drained_finish = drained_flag.clone();
				// The frame stream pulls trailers off to the side (via `arrived`) and yields
				// data bytes only, so decoding sees no trailer frames.
				let bytes = Box::pin(
					BodyStream::new(inner)
						.then(move |frame| {
							let trailers_lock = trailers_stream.clone();
							async move {
								match frame {
									Err(err) => Some(Err(err.to_string())),
									Ok(frame) => match frame.into_trailers() {
										Ok(trailers) => {
											trailers_lock.arrived(trailers);
											None
										}
										Err(frame) => Some(
											frame
												.into_data()
												.map_err(|_| "unknown frame kind".to_string()),
										),
									},
								}
							}
						})
						.filter_map(async |item| item),
				) as Pin<Box<DynStream>>;

				let bytes = match self.decode {
					Some(coding) => decode_stream(bytes, coding),
					None => bytes,
				};

				// A zero-length chunk carries no bytes, but the body's byte-oriented
				// ReadableStream cannot take one: `ReadableByteStreamController.enqueue`
				// rejects an empty buffer outright (`ERR_INVALID_STATE`). Some origins end a
				// response with an empty DATA frame carrying END_STREAM, so drop empty chunks
				// here, before the stream is built, letting it close cleanly. The byte count
				// delivered is unchanged, and the collecting paths (`text()`, `bytes()`) never
				// noticed the empties anyway.
				let bytes = Box::pin(bytes.filter(|item| {
					let empty = matches!(item, Ok(chunk) if chunk.is_empty());
					async move { !empty }
				})) as Pin<Box<DynStream>>;

				// Chained onto the stream that is actually delivered, above any decoder: a
				// decoder reaches the end of its own framing without necessarily polling the
				// bytes underneath to completion, so bookkeeping chained below it would never
				// run for a decoded body, leaving the trailers promise and the timing pending
				// for good.
				let bytes = Box::pin(
					bytes.chain(
						stream::once(async move {
							trailers_finish.ended();
							// The last byte of the body: every read path ends here, so
							// this is where the timing settles
							// (spec:RESP#request-timing).
							timing_finish.ended();
							// Track that we've finished consuming a body
							stats_finish.bodies_finished.fetch_add(1, Ordering::Relaxed);
							// Mark body as drained so Drop doesn't try to drain again
							drained_finish.store(true, Ordering::SeqCst);
						})
						.filter_map(async |()| None),
					),
				) as Pin<Box<DynStream>>;

				let stream = SharedStream::new(bytes);

				// the _ is the Consumed we put in there earlier
				let _ = replace(lock, Body::Stream(stream.clone()));

				Ok(stream)
			}
		}
	}

	/// Underlying efficient response body fetcher.
	///
	/// Unlike bytes() and co, this grabs all the chunks of the response but doesn't
	/// copy them. Further processing is needed to obtain a `Vec<u8>` or whatever is wanted.
	pub async fn gather(&self) -> Result<Arc<[Bytes]>, FaithError> {
		let Some(lock) = &self.body.body else {
			return Ok(Default::default());
		};

		let mut body = lock.lock().await;
		let stream = self.ensure_stream(&mut body, self.body.drained.clone())?;
		drop(body); // release lock before consuming stream

		let mut chunks = Vec::new();
		futures::pin_mut!(stream);
		while let Some(result) = stream.next().await {
			let chunk =
				result.map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)))?;
			chunks.push(chunk);
		}

		// Mark as drained since we consumed everything
		self.body.mark_drained();

		Ok(Arc::from(chunks.into_boxed_slice()))
	}

	/// gather() and then copy into one contiguous buffer
	pub async fn gather_contiguous(&self) -> Result<Vec<u8>, FaithError> {
		let body = self.gather().await?;
		let length = body.iter().map(|chunk| chunk.len()).sum();
		let mut bytes = Vec::with_capacity(length);
		for chunk in body.into_iter() {
			bytes.extend_from_slice(chunk);
		}

		if let Some(ref integrity) = self.integrity {
			verify_integrity(&bytes, integrity)?;
		}

		Ok(bytes)
	}

	/// Write the body out to a file, reporting progress as the bytes land.
	///
	/// `on_progress` is called with the bytes written so far and the advertised length where one is
	/// known, no more often than [`PROGRESS_INTERVAL`], and once more when the last byte is written.
	// spec:BODY#tofile
	pub async fn write_to_file(
		&self,
		path: &str,
		options: &FileDestination,
		mut on_progress: impl FnMut(FileProgress),
	) -> Result<FileWritten, FaithError> {
		// A response that cannot carry a body has nothing to write, and this is settled
		// before any file is created (spec:BODY#tofile).
		let Some(lock) = self.body.body.clone() else {
			return Err(FaithErrorKind::ResponseBodyNull.into());
		};

		// A body already read, or whose stream was handed out, has no second read to give.
		// Checked without committing so an open failure below still leaves the body
		// undisturbed and the caller free to retry to another path.
		if self.disturbed.load(Ordering::SeqCst) {
			return Err(FaithErrorKind::ResponseAlreadyDisturbed.into());
		}

		// Reject a malformed integrity value up front, before the body is touched, the same
		// as the other verified reads reject it when the whole body is in hand.
		let mut checker = integrity_checker(self.integrity.as_deref())?;

		// The advertised length, when the server sent one. It is only visible here for a body
		// delivered as received: a decoded body has had its Content-Length stripped, so the
		// bytes written equal the wire bytes wherever this is Some (spec:BODY#tofile, ENC).
		let content_length = self
			.headers
			.get(CONTENT_LENGTH)
			.and_then(|value| value.to_str().ok())
			.and_then(|value| value.trim().parse::<u64>().ok());

		// The destination is opened before any of the body is read, so a failure to open it
		// leaves the body unread and undisturbed.
		let mut file = open_destination(path, options).await?;

		// Commit the read now the destination is in hand. A concurrent read that slipped in
		// since the load above wins, and this one finds the body already spent.
		self.check_stream_disturbed()?;

		let stream = {
			let mut body = lock.lock().await;
			let stream = self.ensure_stream(&mut body, self.body.drained.clone())?;
			drop(body); // release lock before consuming stream
			stream
		};

		// Reporting is rate limited rather than per chunk, so a large body does not cross a
		// surface boundary thousands of times.
		// spec:BODY#tofile
		let mut report = |written: u64| {
			on_progress(FileProgress {
				bytes_written: written,
				content_length,
			});
		};

		let mut written: u64 = 0;
		let mut reported_at = Instant::now();
		futures::pin_mut!(stream);
		while let Some(result) = stream.next().await {
			let chunk =
				result.map_err(|err| FaithError::new(FaithErrorKind::BodyStream, Some(err)))?;
			if let Some(checker) = checker.as_mut() {
				checker.input(&chunk);
			}
			file.write_all(&chunk)
				.await
				.map_err(|err| FaithError::new(FaithErrorKind::FileWrite, Some(err.to_string())))?;
			written += chunk.len() as u64;
			// A server cannot send more than it promised: once the bytes off the wire exceed
			// the advertised length, the write fails and the bytes so far stay on disk
			// (spec:BODY#tofile).
			if let Some(limit) = content_length {
				if written > limit {
					return Err(FaithErrorKind::ContentLengthOverrun.into());
				}
			}
			if reported_at.elapsed() >= PROGRESS_INTERVAL {
				reported_at = Instant::now();
				report(written);
			}
		}

		file.flush()
			.await
			.map_err(|err| FaithError::new(FaithErrorKind::FileWrite, Some(err.to_string())))?;

		// The last report always lands, whatever the rate limit allowed along the way, so a
		// caller's final view of a completed write is the whole body rather than the last
		// interval boundary. An empty body reports once, with nothing written.
		report(written);

		// The digest is only known once the last byte has been written, so the file that
		// fails verification is on disk when the error arrives (spec:SRI).
		if let Some(checker) = checker {
			finish_integrity(checker)?;
		}

		self.body.mark_drained();

		Ok(FileWritten {
			// A relative path resolves against the process's working directory; the caller
			// is handed the absolute path the bytes landed at.
			path: std::path::absolute(path)
				.map(|abs| abs.to_string_lossy().into_owned())
				.unwrap_or_else(|_| path.to_owned()),
			bytes_written: written,
		})
	}
}
