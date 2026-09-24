//! Response bodies.
//!
//! A body has two parts that are released separately. The upstream is the transfer itself: the
//! HTTP/1 connection, or the HTTP/2 or HTTP/3 stream, the bytes arrive over. The chain is the
//! chunks already received, shared between a response and its clones so each can read the whole
//! body from one transfer.
//!
//! Each response and clone holds a [`Claim`] on the body, with its own position in the chain.
//! Giving a claim up releases that position, so chunks no remaining claim needs are freed, and
//! the last claim to go stops the upstream: dropped on HTTP/2 and HTTP/3, read out within the
//! agent's drain limits on HTTP/1 so its connection can go back to the pool, or dropped past them.

// spec:BODY#giving-up-the-body spec:POOL#draining-abandoned-http-1-bodies

use std::{
	fmt::Debug,
	mem::replace,
	pin::Pin,
	sync::{
		Arc, Mutex, MutexGuard, PoisonError,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
	task::{Context, Poll},
	time::Duration,
};

use bytes::Bytes;
use futures::{Stream, StreamExt, stream, task::AtomicWaker};
use http_body::{Body as _, Frame};
use http_body_util::BodyExt;
use reqwest::Version;
use stream_shared::SharedStream;
use tokio::{runtime::Handle, sync::watch};

#[cfg(feature = "encoding")]
use web_faith_encoding::{Coding, response::decode_stream};

use crate::{
	error::{FaithError, FaithErrorKind},
	response::TrailersSlot,
	stats::InnerAgentStats,
	timing::TimingSlot,
};

/// A body byte-stream, as the shared chain carries one.
pub type DynStream = dyn Stream<Item = std::result::Result<Bytes, String>> + Send + Sync;

/// The chunks of a body, shared between the claims reading it.
type Chain = SharedStream<Pin<Box<DynStream>>>;

/// How much of an abandoned HTTP/1 body is worth reading out to save its connection.
// spec:POOL#draining-abandoned-http-1-bodies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainPolicy {
	/// The most of the remainder read out, in bytes off the wire.
	pub limit: u64,
	/// How long reading it out may take.
	pub timeout: Duration,
}

impl Default for DrainPolicy {
	fn default() -> Self {
		Self {
			limit: 128 * 1024,
			timeout: Duration::from_secs(1),
		}
	}
}

/// Where the transfer stands.
enum Upstream {
	/// Still arriving.
	Live(reqwest::Body),
	/// Stopped before its end; anything still reading gets an error.
	Stopped,
	/// Nothing more will arrive: read to its end, failed, or past a stop's error.
	Ended,
}

/// The state a response and all its clones share for one body.
pub struct BodyShared {
	upstream: Mutex<Upstream>,
	/// Whoever last polled the upstream, woken when it is stopped from outside.
	upstream_waker: AtomicWaker,
	/// Claims not yet given up.
	claims: AtomicUsize,
	/// Set by an abort, so every reader errors at once rather than reading out what is buffered.
	aborted: AtomicBool,
	/// Whether a reader has been opened, which is what the bodies-started counter counts.
	started: AtomicBool,
	/// Whether the body's ending has been recorded, so it is recorded once.
	finished: AtomicBool,
	/// True once the upstream is done with: read to its end, dropped, or drained.
	settled: watch::Sender<bool>,
	version: Version,
	drain: DrainPolicy,
	/// Captured when the response was built. The last claim can be given up from a JS finaliser,
	/// which runs outside any runtime, and an HTTP/1 drain still needs one to run on.
	runtime: Option<Handle>,
	trailers: Arc<TrailersSlot>,
	timing: Arc<TimingSlot>,
	stats: Arc<InnerAgentStats>,
}

impl Debug for BodyShared {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BodyShared")
			.field("claims", &self.claims.load(Ordering::SeqCst))
			.field("aborted", &self.aborted.load(Ordering::SeqCst))
			.field("version", &self.version)
			.field("drain", &self.drain)
			.finish_non_exhaustive()
	}
}

/// Everything a body needs to be built.
pub(crate) struct BodyParts {
	pub body: reqwest::Body,
	pub version: Version,
	pub drain: DrainPolicy,
	#[cfg(feature = "encoding")]
	pub decode: Option<Coding>,
	pub trailers: Arc<TrailersSlot>,
	pub timing: Arc<TimingSlot>,
	pub stats: Arc<InnerAgentStats>,
}

impl BodyShared {
	/// Build a body and the first claim on it, which belongs to the response being built.
	pub(crate) fn new(parts: BodyParts) -> Arc<Claim> {
		let shared = Arc::new(Self {
			upstream: Mutex::new(Upstream::Live(parts.body)),
			upstream_waker: AtomicWaker::new(),
			claims: AtomicUsize::new(1),
			aborted: AtomicBool::new(false),
			started: AtomicBool::new(false),
			finished: AtomicBool::new(false),
			settled: watch::channel(false).0,
			version: parts.version,
			drain: parts.drain,
			runtime: Handle::try_current().ok(),
			trailers: parts.trailers,
			timing: parts.timing,
			stats: parts.stats,
		});

		let chain = SharedStream::new(Self::pipeline(
			&shared,
			#[cfg(feature = "encoding")]
			parts.decode,
		));

		Arc::new(Claim {
			body: shared,
			cursor: Mutex::new(Some(chain)),
			given_up: AtomicBool::new(false),
			waker: AtomicWaker::new(),
		})
	}

	/// The stream of body bytes a reader sees, built over the upstream.
	fn pipeline(
		shared: &Arc<Self>,
		#[cfg(feature = "encoding")] decode: Option<Coding>,
	) -> Pin<Box<DynStream>> {
		// The frame stream pulls trailers off to the side and yields data bytes only, so
		// decoding sees no trailer frames.
		let trailers = shared.trailers.clone();
		let bytes = Box::pin(
			UpstreamFrames {
				shared: shared.clone(),
			}
			.filter_map(move |frame| {
				let item = match frame {
					Err(err) => Some(Err(err)),
					Ok(frame) => match frame.into_trailers() {
						Ok(headers) => {
							trailers.arrived(headers);
							None
						}
						Err(frame) => Some(
							frame
								.into_data()
								.map_err(|_| "unknown frame kind".to_string()),
						),
					},
				};
				async move { item }
			}),
		) as Pin<Box<DynStream>>;

		#[cfg(feature = "encoding")]
		let bytes = match decode {
			Some(coding) => decode_stream(bytes, coding),
			None => bytes,
		};

		// A zero-length chunk carries no bytes, but the body's byte-oriented ReadableStream
		// cannot take one: `ReadableByteStreamController.enqueue` rejects an empty buffer
		// outright. Some origins end a response with an empty DATA frame carrying END_STREAM,
		// so drop empty chunks here, letting the stream close cleanly.
		let bytes = Box::pin(bytes.filter(|item| {
			let empty = matches!(item, Ok(chunk) if chunk.is_empty());
			async move { !empty }
		})) as Pin<Box<DynStream>>;

		// Chained onto the stream that is actually delivered, above any decoder: a decoder
		// reaches the end of its own framing without necessarily polling the bytes underneath
		// to completion, so bookkeeping chained below it would never run for a decoded body.
		let finish = shared.clone();
		Box::pin(
			bytes.chain(
				stream::once(async move {
					// The last byte of the body: every read path ends here (spec:RESP#request-timing).
					finish.finish();
				})
				.filter_map(async |()| None),
			),
		)
	}

	fn upstream(&self) -> MutexGuard<'_, Upstream> {
		self.upstream.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// Record that the body has ended, once, however it ended.
	///
	/// The trailers and the timing settle, and a body that was opened for reading counts as
	/// finished (spec:OBS#stats).
	fn finish(&self) {
		if self.finished.swap(true, Ordering::SeqCst) {
			return;
		}
		self.trailers.ended();
		self.timing.ended();
		if self.started.load(Ordering::SeqCst) {
			self.stats.bodies_finished.fetch_add(1, Ordering::Relaxed);
		}
	}

	/// Note that a reader has been opened on this body.
	fn opened(&self) {
		if !self.started.swap(true, Ordering::SeqCst) {
			self.stats.bodies_started.fetch_add(1, Ordering::Relaxed);
		}
	}

	/// Stop the transfer: nothing reads from the upstream again.
	///
	/// Stopping does not wait for anything to poll the body, so an endless body costs nothing
	/// once no one wants it.
	// spec:BODY#giving-up-the-body
	fn stop(self: &Arc<Self>) {
		let taken = {
			let mut upstream = self.upstream();
			match replace(&mut *upstream, Upstream::Stopped) {
				Upstream::Live(body) => Some(body),
				other => {
					*upstream = other;
					None
				}
			}
		};
		// Whatever is waiting on the next frame finds the stop and errors.
		self.upstream_waker.wake();
		self.finish();

		let Some(body) = taken else {
			self.settled.send_replace(true);
			return;
		};

		// HTTP/2 and HTTP/3 multiplex, so dropping the body resets its stream and leaves the
		// connection alone. An HTTP/1 connection is only reusable once the body has been read
		// to its end, which is worth doing for a small remainder.
		let http1 = matches!(
			self.version,
			Version::HTTP_09 | Version::HTTP_10 | Version::HTTP_11
		);
		match (&self.runtime, http1) {
			(Some(runtime), true) => {
				let shared = self.clone();
				runtime.spawn(async move {
					drain(body, shared.drain).await;
					shared.settled.send_replace(true);
				});
			}
			_ => {
				drop(body);
				self.settled.send_replace(true);
			}
		}
	}

	/// Stop the transfer because the request was aborted, erroring every reader.
	// spec:CANCEL#abortsignal
	#[cfg_attr(not(feature = "internals"), allow(dead_code))]
	pub(crate) fn abort(self: &Arc<Self>) {
		if !self.aborted.swap(true, Ordering::SeqCst) {
			self.stop();
		}
	}

	/// How many claims on the body have not been given up.
	pub(crate) fn claims_left(&self) -> usize {
		self.claims.load(Ordering::SeqCst)
	}

	/// Wait until the upstream is done with.
	pub(crate) async fn settled(&self) {
		let mut rx = self.settled.subscribe();
		let _ = rx.wait_for(|settled| *settled).await;
	}
}

/// Read out an abandoned HTTP/1 body within the drain limits, so its connection can go back to
/// the pool. Returning without reaching the end drops the body, which closes the connection.
// spec:POOL#draining-abandoned-http-1-bodies
async fn drain(mut body: reqwest::Body, policy: DrainPolicy) {
	// A remainder the response's length already shows to be over the limit is not worth
	// starting on.
	if policy.limit == 0 || body.size_hint().lower() > policy.limit {
		return;
	}

	let _ = tokio::time::timeout(policy.timeout, async {
		let mut read: u64 = 0;
		while let Some(frame) = body.frame().await {
			let Ok(frame) = frame else {
				return;
			};
			if let Some(data) = frame.data_ref() {
				read += data.len() as u64;
				if read > policy.limit {
					return;
				}
			}
		}
	})
	.await;
}

/// The upstream's frames, read through the slot a stop can empty.
struct UpstreamFrames {
	shared: Arc<BodyShared>,
}

impl Stream for UpstreamFrames {
	type Item = Result<Frame<Bytes>, String>;

	fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.shared.upstream_waker.register(cx.waker());
		let mut upstream = self.shared.upstream();
		match &mut *upstream {
			Upstream::Live(body) => match Pin::new(body).poll_frame(cx) {
				Poll::Pending => Poll::Pending,
				Poll::Ready(Some(Ok(frame))) => Poll::Ready(Some(Ok(frame))),
				Poll::Ready(Some(Err(err))) => {
					// A failed transfer cannot be resumed, so the body goes now, and an HTTP/1
					// connection with it.
					*upstream = Upstream::Ended;
					drop(upstream);
					self.shared.settled.send_replace(true);
					Poll::Ready(Some(Err(err.to_string())))
				}
				Poll::Ready(None) => {
					*upstream = Upstream::Ended;
					drop(upstream);
					self.shared.settled.send_replace(true);
					Poll::Ready(None)
				}
			},
			Upstream::Stopped => {
				// Stopped before its end, so what was received is not the whole body, and a
				// reader must not mistake it for one.
				*upstream = Upstream::Ended;
				Poll::Ready(Some(Err("the transfer was stopped".to_string())))
			}
			Upstream::Ended => Poll::Ready(None),
		}
	}
}

/// One response's claim on a body, shared by the in-process copies of that response.
///
/// A response and each of its clones hold one claim; the transfer continues for as long as any
/// claim does. Dropping the claim gives it up.
// spec:BODY#giving-up-the-body
pub struct Claim {
	body: Arc<BodyShared>,
	/// This response's position in the chain. `None` once given up.
	cursor: Mutex<Option<Chain>>,
	given_up: AtomicBool,
	/// A reader waiting on the chain, woken when the claim is given up.
	waker: AtomicWaker,
}

impl Debug for Claim {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Claim")
			.field("body", &self.body)
			.field("given_up", &self.given_up.load(Ordering::SeqCst))
			.finish_non_exhaustive()
	}
}

impl Claim {
	fn cursor(&self) -> MutexGuard<'_, Option<Chain>> {
		self.cursor.lock().unwrap_or_else(PoisonError::into_inner)
	}

	/// A claim for a clone of this response, reading from the same position.
	///
	/// `None` if this claim has already been given up, leaving nothing to copy.
	pub(crate) fn duplicate(&self) -> Option<Arc<Self>> {
		let cursor = self.cursor();
		let chain = cursor.as_ref()?.clone();
		self.body.claims.fetch_add(1, Ordering::SeqCst);
		Some(Arc::new(Self {
			body: self.body.clone(),
			cursor: Mutex::new(Some(chain)),
			given_up: AtomicBool::new(false),
			waker: AtomicWaker::new(),
		}))
	}

	/// Whether this claim has been given up.
	pub(crate) fn is_given_up(&self) -> bool {
		self.given_up.load(Ordering::SeqCst)
	}

	/// The body this claim is on.
	pub(crate) fn body(&self) -> &Arc<BodyShared> {
		&self.body
	}

	/// Give the claim up, releasing this response's hold on the chunks already received.
	///
	/// The last claim to go stops the transfer. Returns whether this was that one. Idempotent.
	pub(crate) fn give_up(&self) -> bool {
		if self.given_up.swap(true, Ordering::SeqCst) {
			return false;
		}

		// Dropped outside the lock: releasing the last position frees chunks, which is not
		// work to do while holding it.
		let cursor = self.cursor().take();
		drop(cursor);
		self.waker.wake();

		if self.body.claims.fetch_sub(1, Ordering::SeqCst) == 1 {
			self.body.stop();
			true
		} else {
			false
		}
	}

	/// A reader on this claim's position in the body.
	pub(crate) fn reader(self: &Arc<Self>) -> Result<BodyReader, FaithError> {
		if self.is_given_up() {
			return Err(FaithErrorKind::ResponseAlreadyDisturbed.into());
		}
		self.body.opened();
		Ok(BodyReader {
			claim: self.clone(),
			done: false,
		})
	}
}

impl Drop for Claim {
	fn drop(&mut self) {
		self.give_up();
	}
}

/// A response's body as a stream of chunks.
///
/// Every reader a response hands out reads from that response's one position, so a reader
/// taken after part of the body was read continues from there. Dropping a reader gives the
/// response's claim on the body up (see [`Claim`]).
// spec:BODY#the-body-stream
pub struct BodyReader {
	claim: Arc<Claim>,
	done: bool,
}

impl Debug for BodyReader {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BodyReader")
			.field("claim", &self.claim)
			.field("done", &self.done)
			.finish()
	}
}

impl BodyReader {
	/// A handle that gives this reader's claim up without holding the reader, for a caller
	/// whose reader is busy on a read when it wants to cancel. Permanently unstable.
	#[cfg(feature = "internals")]
	pub fn canceller(&self) -> BodyCanceller {
		BodyCanceller(self.claim.clone())
	}

	fn fail(&mut self, kind: FaithErrorKind) -> Poll<Option<Result<Bytes, FaithError>>> {
		self.done = true;
		Poll::Ready(Some(Err(kind.into())))
	}
}

impl Stream for BodyReader {
	type Item = Result<Bytes, FaithError>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		if self.done {
			return Poll::Ready(None);
		}

		self.claim.waker.register(cx.waker());
		// An abort errors every reader at once, ahead of chunks it has not read yet
		// (spec:CANCEL#abortsignal).
		if self.claim.body.aborted.load(Ordering::SeqCst) {
			return self.fail(FaithErrorKind::Aborted);
		}

		let polled = {
			let mut cursor = self.claim.cursor();
			match cursor.as_mut() {
				// Given up, by `discard()` or a cancel, while this reader was open.
				None => None,
				Some(chain) => Some(Pin::new(chain).poll_next(cx)),
			}
		};

		match polled {
			None => self.fail(FaithErrorKind::ResponseAlreadyDisturbed),
			Some(Poll::Pending) => Poll::Pending,
			Some(Poll::Ready(None)) => {
				self.done = true;
				Poll::Ready(None)
			}
			Some(Poll::Ready(Some(Ok(chunk)))) => Poll::Ready(Some(Ok(chunk))),
			Some(Poll::Ready(Some(Err(err)))) => {
				// The chain carries a stop as an error; an abort is reported as one.
				if self.claim.body.aborted.load(Ordering::SeqCst) {
					return self.fail(FaithErrorKind::Aborted);
				}
				self.done = true;
				Poll::Ready(Some(Err(FaithError::new(FaithErrorKind::BodyStream, err))))
			}
		}
	}
}

impl Drop for BodyReader {
	fn drop(&mut self) {
		self.claim.give_up();
	}
}

/// Gives a body reader's claim up from outside the reader. Permanently unstable.
#[cfg(feature = "internals")]
#[derive(Debug, Clone)]
pub struct BodyCanceller(Arc<Claim>);

#[cfg(feature = "internals")]
impl BodyCanceller {
	/// Give the claim up, as dropping the reader would.
	pub fn cancel(&self) {
		self.0.give_up();
	}
}
