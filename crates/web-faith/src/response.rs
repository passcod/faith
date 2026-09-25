//! Responses, bodies, and request timing.

pub use crate::timing::RequestTiming;

// spec:RESP spec:TRL spec:BODY

use std::{
	fmt::Debug,
	net::SocketAddr,
	path::{Path, PathBuf},
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	task::{Context, Poll},
	time::{Duration, Instant},
};

use bytes::Bytes;
use futures::{Stream, StreamExt, stream};
use http::header::{CONTENT_LENGTH, HeaderMap};
use reqwest::{StatusCode, Url, Version};
use serde::de::DeserializeOwned;
use tokio::{io::AsyncWriteExt, sync::watch};

pub use crate::body::BodyReader;
use crate::{
	body::Claim,
	error::{FaithError, FaithErrorKind},
	timing::TimingSlot,
};

use crate::integrity::{finish_integrity, integrity_checker, verify_integrity};

/// The peer that sent a response.
#[derive(Debug)]
pub struct PeerInformation {
	/// The peer's address and port, where the connection could report one.
	pub address: Option<SocketAddr>,
	/// The peer's DER-encoded leaf certificate, for a response that arrived over HTTPS.
	pub certificate: Option<Vec<u8>>,
}

/// Where a response body is written.
#[derive(Debug, Clone, Default)]
pub struct FileDestination {
	/// Truncate and replace an occupied destination. The default refuses one instead, leaving what
	/// is there untouched.
	pub overwrite: bool,
	/// The permissions a newly created file is given. Ignored on platforms without Unix file modes.
	pub mode: Option<u32>,
}

// Reporting every chunk would cross the surface boundary thousands of times for a large body,
// which is the cost writing to a file directly exists to avoid. The final report always lands
// regardless.
pub(crate) const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// Open the destination file for a body write, mapping filesystem refusals to Faith's errors.
// spec:BODY#tofile
pub(crate) async fn open_destination(
	path: &Path,
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

/// Classify a failure to open the destination: `FileExists` for an occupied path, `FileWrite` for
/// a directory or any other refusal, carrying the OS detail.
pub(crate) async fn classify_open_error(path: &Path, err: std::io::Error) -> FaithError {
	let kind = if err.kind() == std::io::ErrorKind::AlreadyExists {
		match tokio::fs::symlink_metadata(path).await {
			Ok(meta) if meta.is_dir() => FaithErrorKind::FileWrite,
			_ => FaithErrorKind::FileExists,
		}
	} else {
		FaithErrorKind::FileWrite
	};
	FaithError::new(kind, err.to_string())
}

/// The trailing headers a response carried, once its body has ended.
#[derive(Clone, Debug, Default)]
pub enum Trailers {
	/// The body has not ended, so the question is still open.
	#[default]
	NotYet,
	/// The body ended carrying no trailers.
	None,
	/// The trailers that arrived.
	Some(HeaderMap),
}

/// Where the trailers land: written by whoever finishes the body, awaited by `trailers()`.
///
/// A watch channel, so waiting parks instead of spinning. The fetch standard's trailers proposal
/// (<https://github.com/whatwg/fetch/pull/1940>) has this not resolving until the body is
/// consumed, so the wait is unbounded by design.
#[derive(Debug)]
pub(crate) struct TrailersSlot(watch::Sender<Trailers>);

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
	/// `send_if_modified` keeps the read and write one step, and wakes waiters only from the call
	/// that settled it.
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

	/// A response that cannot carry a body converts to an `http::Response` with an empty one,
	/// carrying its status, version, and headers across.
	#[test]
	fn a_bodyless_response_converts_to_an_http_response() {
		let mut headers = HeaderMap::new();
		headers.insert("x-test", "yes".parse().expect("a valid header value"));

		let response = Response {
			claim: None,
			disturbed: Arc::new(AtomicBool::new(false)),
			headers,
			integrity: None,
			peer: Arc::new(PeerInformation {
				address: None,
				certificate: None,
			}),
			redirected: false,
			status_code: StatusCode::NO_CONTENT,
			timing: Arc::new(TimingSlot::new(
				Instant::now(),
				crate::timing::RequestTiming::default(),
			)),
			trailers: Arc::new(TrailersSlot::default()),
			url: Url::parse("https://example.com/").expect("a valid url"),
			version: Version::HTTP_2,
		};

		let http = response.into_http().expect("an undisturbed body converts");
		assert_eq!(http.status(), StatusCode::NO_CONTENT);
		assert_eq!(http.version(), Version::HTTP_2);
		assert_eq!(
			http.headers().get("x-test").map(|v| v.as_bytes()),
			Some(&b"yes"[..])
		);

		// Draining it yields nothing, which is what a consumer of the body actually observes.
		let collected =
			futures::executor::block_on(http_body_util::BodyExt::collect(http.into_body()))
				.expect("an empty body collects");
		assert!(collected.to_bytes().is_empty());
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
#[non_exhaustive]
pub struct FileProgress {
	/// Bytes written to the file so far.
	pub bytes_written: u64,
	/// The `Content-Length` the response advertised. Absent for a chunked response, and for one
	/// being decoded, where the final size is not known ahead of time.
	pub content_length: Option<u64>,
}

/// The result of writing a body to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileWritten {
	/// The absolute filesystem path written to.
	pub path: PathBuf,
	/// The number of bytes that landed at the destination.
	pub bytes_written: u64,
}

/// A response to a request.
///
/// Arrives from a request; it is not constructed directly. Reading the body consumes it, so a
/// second read fails. [`Self::try_clone`] gets a copy that can be read separately.
#[derive(Debug, Clone)]
pub struct Response {
	/// This response's claim on the body, shared by its in-process copies and given up when the
	/// last of them goes. `None` for a response that cannot carry a body.
	// spec:BODY#giving-up-the-body
	pub(crate) claim: Option<Arc<Claim>>,
	pub(crate) disturbed: Arc<AtomicBool>,
	pub(crate) headers: HeaderMap,
	pub(crate) integrity: Option<String>,
	pub(crate) peer: Arc<PeerInformation>,
	pub(crate) redirected: bool,
	pub(crate) status_code: StatusCode,
	pub(crate) timing: Arc<TimingSlot>,
	pub(crate) trailers: Arc<TrailersSlot>,
	pub(crate) url: Url,
	pub(crate) version: Version,
}

impl Response {
	/// The response's status.
	pub fn status(&self) -> StatusCode {
		self.status_code
	}

	/// The canonical reason phrase for the status, or empty for a code with no well-known one.
	///
	/// Always the canonical phrase. HTTP/1 lets a server send its own, which is not surfaced here;
	/// HTTP/2 and HTTP/3 carry none at all.
	pub fn status_text(&self) -> &'static str {
		self.status_code.canonical_reason().unwrap_or_default()
	}

	/// Whether the status is in the 2xx range.
	pub fn ok(&self) -> bool {
		self.status_code.is_success()
	}

	/// The response's headers.
	pub fn headers(&self) -> &HeaderMap {
		&self.headers
	}

	/// The URL the response came from, after any redirects.
	pub fn url(&self) -> &Url {
		&self.url
	}

	/// Whether a redirect was followed to reach this response.
	pub fn redirected(&self) -> bool {
		self.redirected
	}

	/// The HTTP version the response arrived over.
	pub fn version(&self) -> Version {
		self.version
	}

	/// The peer that sent the response.
	pub fn peer(&self) -> &PeerInformation {
		&self.peer
	}

	/// Copy the response, so the body can be read twice.
	///
	/// Both copies read the same underlying body, and neither is disturbed by the other having
	/// been cloned. Fails if the body has already been read.
	pub fn try_clone(&self) -> Result<Self, FaithError> {
		// A read, not `check_stream_disturbed`: that one swaps the flag, which would disturb the
		// response being cloned and leave neither copy readable.
		if self.body_used() {
			return Err(FaithErrorKind::ResponseAlreadyDisturbed.into());
		}

		// The copy holds a claim of its own, so it keeps the transfer going after this one
		// gives up. A discarded body has no claim left to copy.
		let claim = match &self.claim {
			None => None,
			Some(claim) => Some(
				claim
					.duplicate()
					.ok_or(FaithErrorKind::ResponseAlreadyDisturbed)?,
			),
		};

		Ok(Self {
			claim,
			disturbed: Arc::new(AtomicBool::new(false)),
			..Clone::clone(self)
		})
	}

	/// Whether the body has been read, or handed out as a stream.
	pub fn body_used(&self) -> bool {
		self.disturbed.load(Ordering::SeqCst)
	}

	/// Read the whole body.
	///
	/// Consumes it, so a second read fails. A request's `integrity` is verified here, once the
	/// whole body is in hand.
	// spec:BODY
	pub async fn bytes(&self) -> Result<Vec<u8>, FaithError> {
		self.check_stream_disturbed()?;
		self.gather_contiguous().await
	}

	/// Read the whole body as text.
	///
	/// Decoded as UTF-8, with invalid sequences replaced by U+FFFD.
	pub async fn text(&self) -> Result<String, FaithError> {
		let bytes = self.bytes().await?;
		Ok(String::from_utf8(bytes)
			.unwrap_or_else(|err| String::from_utf8_lossy(err.as_bytes()).into_owned()))
	}

	/// Read the whole body and deserialise it from JSON.
	///
	/// Reads into memory before parsing, which can cost twice the body's size; [`Self::body_stream`]
	/// avoids that.
	pub async fn json<T: DeserializeOwned>(&self) -> Result<T, FaithError> {
		let bytes = self.bytes().await?;
		serde_json::from_slice(&bytes)
			.map_err(|err| FaithError::new(FaithErrorKind::JsonParse, err.to_string()))
	}

	/// The body as a stream of chunks, decoded under whichever coding was negotiated.
	///
	/// `None` for a response that cannot carry a body. Callable more than once: every stream a
	/// response hands out reads from its one position in the body, so one taken after part of
	/// the body was read continues from there. Dropping a stream gives the response's claim on
	/// the body up, stopping the transfer once no clone still wants it. Fails if the body has
	/// already been read to the end or discarded.
	// spec:BODY
	pub fn body_stream(&self) -> Result<Option<BodyReader>, FaithError> {
		// The body counts as disturbed from here, though the stream itself stays re-readable.
		let _ = self.check_stream_disturbed();

		match &self.claim {
			None => Ok(None),
			Some(claim) => claim.reader().map(Some),
		}
	}

	/// Give up on the body, releasing the connection back to the pool.
	///
	/// Worth doing when the body is not wanted: left unread, the claim on it is held until the
	/// response drops. Gives up this response's claim only, so a clone still reading carries on;
	/// when it was the last claim, resolves once the transfer has been stopped: the stream reset
	/// on HTTP/2 and HTTP/3, the connection drained back to the pool or closed on HTTP/1.
	// spec:BODY#discard spec:BODY#giving-up-the-body
	pub async fn discard(&self) {
		let Some(claim) = &self.claim else {
			return;
		};
		claim.give_up();
		if claim.body().claims_left() == 0 {
			claim.body().settled().await;
		}
	}

	/// The timing of the request that produced this response, once its body has ended.
	// spec:RESP#request-timing
	pub async fn timing(&self) -> crate::timing::RequestTiming {
		self.timing.settled().await
	}

	/// The trailers, once the body has ended.
	///
	/// A body that is never read never ends, so this waits indefinitely; see [`Trailers`].
	// spec:TRL
	pub async fn trailers(&self) -> Trailers {
		self.trailers.settled().await
	}

	pub(crate) fn check_stream_disturbed(&self) -> Result<(), FaithError> {
		if self.disturbed.swap(true, Ordering::SeqCst) {
			Err(FaithErrorKind::ResponseAlreadyDisturbed.into())
		} else {
			Ok(())
		}
	}

	/// Read the whole body as the chunks it arrived in, without copying them.
	///
	/// [`Self::bytes`] and its siblings are built on this.
	pub(crate) async fn gather(&self) -> Result<Arc<[Bytes]>, FaithError> {
		let Some(claim) = &self.claim else {
			return Ok(Default::default());
		};

		let mut stream = claim.reader()?;
		let mut chunks = Vec::new();
		while let Some(chunk) = stream.next().await {
			chunks.push(chunk?);
		}

		Ok(Arc::from(chunks.into_boxed_slice()))
	}

	/// [`Self::gather`], then copy the chunks into one contiguous buffer.
	pub(crate) async fn gather_contiguous(&self) -> Result<Vec<u8>, FaithError> {
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

	/// Write the body to a file, reporting progress as the bytes land.
	///
	/// `on_progress` is called with the bytes written so far and the advertised length when it is
	/// known, at most every 50ms, and once more when the last byte is written.
	// spec:BODY#tofile
	pub async fn write_to_file(
		&self,
		path: impl AsRef<Path>,
		options: &FileDestination,
		mut on_progress: impl FnMut(FileProgress),
	) -> Result<FileWritten, FaithError> {
		let path = path.as_ref();
		// A response that cannot carry a body has nothing to write, and this is settled
		// before any file is created (spec:BODY#tofile).
		let Some(claim) = self.claim.clone() else {
			return Err(FaithErrorKind::ResponseBodyNull.into());
		};

		// A body already read, discarded, or whose stream was handed out, has no second read to
		// give. Checked without committing so an open failure below still leaves the body
		// undisturbed and the caller free to retry to another path.
		if self.disturbed.load(Ordering::SeqCst) || claim.is_given_up() {
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

		let stream = claim.reader()?;

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
			let chunk = result?;
			if let Some(checker) = checker.as_mut() {
				checker.input(&chunk);
			}
			file.write_all(&chunk)
				.await
				.map_err(|err| FaithError::new(FaithErrorKind::FileWrite, err.to_string()))?;
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
			.map_err(|err| FaithError::new(FaithErrorKind::FileWrite, err.to_string()))?;

		// The last report always lands, whatever the rate limit allowed along the way, so a
		// caller's final view of a completed write is the whole body rather than the last
		// interval boundary. An empty body reports once, with nothing written.
		report(written);

		// The digest is only known once the last byte has been written, so the file that
		// fails verification is on disk when the error arrives (spec:SRI).
		if let Some(checker) = checker {
			finish_integrity(checker)?;
		}

		Ok(FileWritten {
			// A relative path resolves against the process's working directory; the caller
			// is handed the absolute path the bytes landed at.
			path: std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
			bytes_written: written,
		})
	}
}

/// A response's body plumbing, for a surface that drives it directly. Permanently unstable.
#[cfg(feature = "unstable-internals")]
impl Response {
	/// Whether the body has already been read or handed out, marking it disturbed either way.
	pub fn check_disturbed(&self) -> Result<(), FaithError> {
		self.check_stream_disturbed()
	}

	/// Stop the body's transfer because the request's signal was aborted after its headers
	/// arrived. Every reader of the response and its clones errors with `Aborted`.
	// spec:CANCEL#abortsignal
	pub fn abort_body(&self) {
		if let Some(claim) = &self.claim {
			claim.body().abort();
		}
	}
}

/// A [`Response`]'s body as an [`http_body::Body`].
///
/// For handing to a tower service, a hyper client, or anything else that takes one.
pub struct ResponseBody {
	chunks: Pin<Box<dyn Stream<Item = Result<Bytes, FaithError>> + Send>>,
}

impl Debug for ResponseBody {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ResponseBody").finish_non_exhaustive()
	}
}

impl http_body::Body for ResponseBody {
	type Data = Bytes;
	type Error = FaithError;

	fn poll_frame(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
	) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
		self.chunks
			.as_mut()
			.poll_next(cx)
			.map(|chunk| chunk.map(|chunk| chunk.map(http_body::Frame::data)))
	}
}

impl Response {
	/// Convert into an [`http::Response`], for code written against the wider ecosystem.
	///
	/// Fails if the body is already being consumed elsewhere. A response that cannot carry a body
	/// yields an empty one.
	pub fn into_http(self) -> Result<http::Response<ResponseBody>, FaithError> {
		let chunks: Pin<Box<dyn Stream<Item = Result<Bytes, FaithError>> + Send>> =
			match self.body_stream()? {
				Some(stream) => Box::pin(stream),
				None => Box::pin(stream::empty()),
			};

		let mut response = http::Response::new(ResponseBody { chunks });
		*response.status_mut() = self.status_code;
		*response.version_mut() = self.version;
		*response.headers_mut() = self.headers.clone();
		Ok(response)
	}
}

impl TryFrom<Response> for http::Response<ResponseBody> {
	type Error = FaithError;

	fn try_from(response: Response) -> Result<Self, Self::Error> {
		response.into_http()
	}
}
