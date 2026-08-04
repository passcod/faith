# HTTP/3 Fallback Under Request Cancellation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a broken HTTP/3 path degrade to TCP within a bounded number of retries, no matter how the caller cancels its requests.

**Architecture:** `AltSvcMiddleware` can only learn that HTTP/3 is broken from the h3 attempt's return value, and a cancelled request never produces one — `faith_fetch` races `send()` against the abort signal in a `tokio::select!`, which drops the losing future. Two independent mechanisms fix this: a `Drop` guard that records a cancellation *strike* against the origin (three within a rolling 60s window demote it), and an optional deadline on the h3 attempt that turns a hang into a normal failure-and-fallback.

**Tech Stack:** Rust 2024, napi-rs 3 (`#[napi]` bindings generate `index.d.ts`), reqwest 0.13 + reqwest-middleware 0.5, moka 0.12 sync caches, tape for JS tests.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-08-04-h3-cancellation-fallback-design.md`. Read it before starting.
- **No new dependencies.** In particular, do not add `anyhow` — that is why deadline expiry takes the fallback branch directly instead of synthesising a `reqwest_middleware::Error`.
- **Do not modify `src/fetch.rs`.** The `select!` keeps dropping the future; the guard is what makes that observable.
- The `Drop` impl must be infallible: no panics, no `unwrap`, no `expect`. It can run during unwind, where a panic aborts the process.
- Version control is **jj, not git**. Commit with `jj describe -m "..."` then `jj new`. Do not run `git commit`.
- Work stacks on bookmark `claude/h3-cancellation-fix` (which already holds the design doc, itself stacked on `claude/h3-abort-no-fallback`).
- Indentation in this repo is **tabs**, in both Rust and JS. Match surrounding style.
- All new public `#[napi]` fields need doc comments — they generate `index.d.ts` and the published API docs.
- `npm run build` must be run before any JS test, or the tests load a stale `.node`.

---

### Task 1: Strike tracking in `AltSvcCache`

Pure cache logic with unit tests. No behaviour change yet — nothing calls the new method until Task 2.

**Files:**
- Modify: `src/alt_svc.rs` (struct at 17-25, `new` at 38-60, `confirm_h3` at 127-147, `record_h3_failure` at 149-157, `Debug` at 27-35, `mod tests` at 309+)
- Modify: `src/agent.rs:700-705` (the `AltSvcCache::new` call site)
- Test: `src/alt_svc.rs` `mod tests`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `AltSvcCache::new(advertised_ttl: Duration, confirmed_ttl: Duration, failed_ttl: Duration, capacity: u64, cancel_strikes: u32, strike_window: Duration) -> Self`
  - `AltSvcCache::record_h3_cancellation(&self, url: &reqwest::Url)`

- [ ] **Step 1: Add the field to the struct and the `Debug` impl**

In `src/alt_svc.rs`, add `cancellations` to the struct (after `failed`) and `cancel_strikes` (after `confirmed_ttl`):

```rust
#[derive(Clone)]
pub struct AltSvcCache {
	advertised: Cache<String, AltSvcEntry>,
	confirmed: Cache<String, AltSvcEntry>,
	failed: Cache<String, ()>,
	/// Consecutive cancelled HTTP/3 attempts per origin. Entries expire on a TTL
	/// (the strike window), so a run has to be sustained to count.
	cancellations: Cache<String, u32>,

	advertised_ttl: Duration,
	confirmed_ttl: Duration,
	cancel_strikes: u32,
}
```

Add the count to `Debug` so `FAITH_TRACE`-style debugging shows it:

```rust
impl std::fmt::Debug for AltSvcCache {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcCache")
			.field("advertised_count", &self.advertised.entry_count())
			.field("confirmed_count", &self.confirmed.entry_count())
			.field("failed_count", &self.failed.entry_count())
			.field("cancellation_count", &self.cancellations.entry_count())
			.finish()
	}
}
```

- [ ] **Step 2: Extend the constructor**

Replace `AltSvcCache::new` with:

```rust
	pub fn new(
		advertised_ttl: Duration,
		confirmed_ttl: Duration,
		failed_ttl: Duration,
		capacity: u64,
		cancel_strikes: u32,
		strike_window: Duration,
	) -> Self {
		Self {
			advertised: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(advertised_ttl)
				.build(),
			confirmed: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(confirmed_ttl)
				.build(),
			failed: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(failed_ttl)
				.build(),
			cancellations: Cache::builder()
				.max_capacity(capacity)
				.time_to_live(strike_window)
				.build(),
			advertised_ttl,
			confirmed_ttl,
			cancel_strikes,
		}
	}
```

`strike_window` is a constructor parameter rather than public config so unit tests can pass a short window and assert decay without a time-mocking dependency.

- [ ] **Step 3: Keep `src/agent.rs` compiling**

In `src/agent.rs`, the `AltSvcCache::new` call currently reads:

```rust
			let cache = Arc::new(AltSvcCache::new(
				advertised_ttl,
				confirmed_ttl,
				failed_ttl,
				capacity,
			));
```

Change it to pass the defaults directly. Task 3 replaces these with real options:

```rust
			let cache = Arc::new(AltSvcCache::new(
				advertised_ttl,
				confirmed_ttl,
				failed_ttl,
				capacity,
				3,
				Duration::from_secs(60),
			));
```

- [ ] **Step 4: Write the failing unit tests**

In `src/alt_svc.rs`, replace the existing `test_cache()` helper with a parameterised pair (the existing `test_cache_flow`, `test_cache_failure` and `test_hint` tests keep calling `test_cache()` unchanged):

```rust
	fn test_cache() -> AltSvcCache {
		test_cache_with(3, Duration::from_secs(60))
	}

	fn test_cache_with(cancel_strikes: u32, strike_window: Duration) -> AltSvcCache {
		AltSvcCache::new(
			Duration::from_secs(86400),
			Duration::from_secs(86400),
			Duration::from_secs(300),
			10_000,
			cancel_strikes,
			strike_window,
		)
	}
```

Then add these five tests to `mod tests`:

```rust
	#[test]
	fn test_cancellation_below_threshold_keeps_h3() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

		cache.record_h3_cancellation(&url);
		cache.record_h3_cancellation(&url);

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"two strikes is not enough to demote"
		);
	}

	#[test]
	fn test_cancellation_at_threshold_demotes() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

		for _ in 0..3 {
			cache.record_h3_cancellation(&url);
		}

		assert!(
			cache.should_use_h3(&url).is_none(),
			"three strikes demotes the origin"
		);
		assert!(
			cache
				.failed
				.contains_key(&"https://example.com:443".to_string()),
			"demotion goes through the failed cache, so re-advertisement can't re-arm it"
		);
	}

	#[test]
	fn test_cancellation_reset_by_h3_success() {
		let cache = test_cache();
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

		cache.record_h3_cancellation(&url);
		cache.record_h3_cancellation(&url);
		cache.confirm_h3(&url);
		cache.record_h3_cancellation(&url);
		cache.record_h3_cancellation(&url);

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"a working h3 response clears the strikes, so these two start over"
		);
	}

	#[test]
	fn test_cancellation_disabled_by_zero() {
		let cache = test_cache_with(0, Duration::from_secs(60));
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

		for _ in 0..5 {
			cache.record_h3_cancellation(&url);
		}

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"cancel_strikes: 0 disables cancellation-based demotion"
		);
	}

	#[test]
	fn test_cancellation_strikes_decay() {
		let cache = test_cache_with(3, Duration::from_millis(50));
		let url = reqwest::Url::parse("https://example.com/path").unwrap();
		cache.record_alt_svc(&url, 443, None);

		cache.record_h3_cancellation(&url);
		cache.record_h3_cancellation(&url);
		std::thread::sleep(Duration::from_millis(150));
		cache.record_h3_cancellation(&url);
		cache.record_h3_cancellation(&url);

		assert_eq!(
			cache.should_use_h3(&url),
			Some(443),
			"strikes older than the window don't count towards the run"
		);
	}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test alt_svc`

Expected: FAIL — compile error, `no method named record_h3_cancellation found for struct AltSvcCache`.

- [ ] **Step 6: Implement `record_h3_cancellation`**

Add to `impl AltSvcCache`, after `confirm_h3`:

```rust
	/// Record an HTTP/3 attempt that was cancelled before producing an outcome.
	///
	/// This is weaker evidence than an error: the request never got to find out
	/// whether HTTP/3 worked, so a single cancellation says nothing about the
	/// origin. Only a sustained run of them demotes it, which keeps callers that
	/// routinely abort healthy requests from disabling HTTP/3.
	///
	/// The window is a TTL measured from the *previous* strike, because moka
	/// refreshes an entry's TTL on upsert. Strikes therefore have to arrive
	/// within a window of each other, not within a fixed bucket.
	pub fn record_h3_cancellation(&self, url: &reqwest::Url) {
		if self.cancel_strikes == 0 {
			return;
		}

		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		let strikes = self
			.cancellations
			.entry(origin)
			.and_upsert_with(|existing| existing.map_or(1, |entry| entry.into_value() + 1))
			.into_value();

		if strikes >= self.cancel_strikes {
			// Clears the strike count as a side effect.
			self.record_h3_failure(url);
		}
	}
```

`and_upsert_with` is atomic per key, so concurrent aborts cannot lose increments.

- [ ] **Step 7: Clear strikes on the two outcomes that settle the question**

In `confirm_h3`, add the invalidation just before inserting the confirmed entry:

```rust
		// A working h3 response is proof of health; forget any strikes.
		self.cancellations.invalidate(&origin);

		let entry = AltSvcEntry {
			port,
			expires: Instant::now() + self.confirmed_ttl,
		};

		self.confirmed.insert(origin, entry);
```

In `record_h3_failure`, add it alongside the other invalidations:

```rust
	pub fn record_h3_failure(&self, url: &reqwest::Url) {
		let Some(origin) = Self::origin_key(url) else {
			return;
		};

		self.advertised.invalidate(&origin);
		self.confirmed.invalidate(&origin);
		// Already demoted; further counting is meaningless.
		self.cancellations.invalidate(&origin);
		self.failed.insert(origin, ());
	}
```

Note the ordering constraint in `confirm_h3`: `origin` is moved into `self.confirmed.insert(origin, entry)`, so the `invalidate(&origin)` call must come before it.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test alt_svc`

Expected: PASS, 8 tests in `alt_svc::tests` (3 pre-existing cache tests + 5 new).

- [ ] **Step 9: Commit**

```bash
jj describe -m "feat: track cancelled HTTP/3 attempts as strikes in AltSvcCache

A cancelled h3 attempt is weaker evidence than an error: the request never
found out whether HTTP/3 worked. Records strikes against the origin instead,
demoting only after a sustained run within a rolling window, so callers that
routinely abort healthy requests don't disable HTTP/3.

Nothing calls this yet.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj new
```

---

### Task 2: Drop guard in the middleware

This is the headline fix. It flips the failing test from PR #23 to passing.

**Files:**
- Modify: `src/alt_svc.rs` (add the guard struct near `AltSvcMiddleware`; change the `trying_h3` branch of `handle` at 255-289)
- Modify: `src/agent.rs:675` (read the new option), `src/agent.rs:700-707` (pass it through)
- Modify: `README.md` (after the TTL knobs paragraph at ~690)
- Modify: `index.d.ts` (regenerated, not hand-edited)
- Test: `test/http3-abort-fallback.test.js` (exists, currently failing — no edits needed)

**Interfaces:**
- Consumes: `AltSvcCache::record_h3_cancellation(&self, url: &reqwest::Url)` from Task 1; `AltSvcCache::new(..., cancel_strikes: u32, strike_window: Duration)` from Task 1.
- Produces: `AgentHttp3Options.upgrade_cancel_strikes: Option<u32>` (JS: `upgradeCancelStrikes`).

- [ ] **Step 1: Run the existing test to confirm it fails**

Run: `npm run build && npx tape test/http3-abort-fallback.test.js`

Expected: assertion 1 passes ("precondition: the origin is confirmed as HTTP/3 through the relay"), assertions 2 and 3 FAIL. Requires `caddy` on PATH; install with `go install github.com/caddyserver/caddy/v2/cmd/caddy@v2.11.4` if missing.

- [ ] **Step 2: Add the guard struct**

In `src/alt_svc.rs`, add immediately before `pub struct AltSvcMiddleware`:

```rust
/// Records a cancellation if the HTTP/3 attempt it guards is dropped before
/// producing an outcome.
///
/// [`AltSvcMiddleware`] can only learn that HTTP/3 is broken from the attempt's
/// return value, and a cancelled request never produces one: `faith_fetch`
/// races `send()` against the abort signal in a `select!`, which drops the
/// losing future. Without this guard nothing ever demotes the origin, so a
/// caller whose deadline is shorter than the network's own failure detection
/// re-attempts HTTP/3 over a dead path on every retry, indefinitely.
struct H3AttemptGuard {
	cache: Arc<AltSvcCache>,
	url: reqwest::Url,
	armed: bool,
}

impl H3AttemptGuard {
	fn new(cache: Arc<AltSvcCache>, url: reqwest::Url) -> Self {
		Self {
			cache,
			url,
			armed: true,
		}
	}

	/// The attempt produced an outcome, so it speaks for itself.
	fn disarm(&mut self) {
		self.armed = false;
	}
}

impl Drop for H3AttemptGuard {
	fn drop(&mut self) {
		// Must stay infallible: this can run while unwinding, where a panic
		// would abort the process. moka's sync cache does not panic on insert.
		if self.armed {
			self.cache.record_h3_cancellation(&self.url);
		}
	}
}
```

- [ ] **Step 3: Arm the guard around the h3 attempt**

In `Middleware::handle`, inside `if let Some(req_clone) = req.try_clone() {`, wrap the attempt. Only the two lines around `let result` change:

```rust
			if let Some(req_clone) = req.try_clone() {
				*req.version_mut() = http::Version::HTTP_3;

				let mut guard = H3AttemptGuard::new(Arc::clone(&self.cache), url.clone());
				let result = next.clone().run(req, extensions).await;
				// Reached on success and on error alike; only a mid-flight drop
				// skips it and leaves the guard armed.
				guard.disarm();

				match result {
```

Leave the `match result { ... }` body exactly as it is. The `else` branch (`try_clone` returned `None`, i.e. a streaming body) still skips HTTP/3 entirely and needs no guard.

- [ ] **Step 4: Run the test to verify it passes**

Run: `npm run build && npx tape test/http3-abort-fallback.test.js`

Expected: PASS, 3/3. The 4th retry falls back over HTTP/2 once three strikes accumulate.

- [ ] **Step 5: Expose `upgradeCancelStrikes`**

In `src/agent.rs`, add to `AgentHttp3Options` after the `upgrade_failed_ttl` field:

```rust
	/// How many consecutive cancelled HTTP/3 attempts, within a 60-second window,
	/// demote an origin back to TCP.
	///
	/// Fáith normally learns that HTTP/3 is broken from a failed attempt. A request
	/// cancelled via `AbortSignal` never produces that signal, so without this an
	/// origin whose UDP path breaks keeps being retried over HTTP/3 for as long as
	/// the Alt-Svc entry lives. Cancellations are treated as weak evidence: only a
	/// sustained run of them demotes the origin, and any successful HTTP/3 response
	/// resets the count.
	///
	/// Set to 0 to disable, so only real HTTP/3 errors demote an origin.
	///
	/// Default: 3.
	pub upgrade_cancel_strikes: Option<u32>,
```

Then read it alongside the other options (near `src/agent.rs:675`, inside the `#[cfg(feature = "http3")] let alt_svc_cache = {` block):

```rust
			let cancel_strikes = http3_opts
				.and_then(|o| o.upgrade_cancel_strikes)
				.unwrap_or(3);
```

And pass it to the constructor, replacing the hardcoded values from Task 1 Step 3:

```rust
			let cache = Arc::new(AltSvcCache::new(
				advertised_ttl,
				confirmed_ttl,
				failed_ttl,
				capacity,
				cancel_strikes,
				Duration::from_secs(60),
			));
```

The 60-second strike window stays internal — it is a heuristic detail, not a knob worth exposing.

- [ ] **Step 6: Document it in the README**

In `README.md`, the four TTL knobs share one lumped paragraph ending "These four settings allow tweaking the HTTP/3 advertisement/knowledge cache behaviour." Add a new section immediately after it:

```markdown
#### `AgentOptions.http3.upgradeCancelStrikes: number`

Fáith learns that HTTP/3 is broken from a failed attempt. A request cancelled via `AbortSignal`
never produces that signal — the attempt is abandoned before it can fail — so a cancelled attempt
would otherwise teach Fáith nothing, and an origin whose UDP path has broken would keep being
retried over HTTP/3 for as long as its Alt-Svc entry lives.

Cancellations are therefore counted as weak evidence: this many of them in a row, each within a
minute of the last, demote the origin to TCP for `upgradeFailedTtl`. Any successful HTTP/3 response
resets the count, so aborting healthy requests doesn't accumulate.

Set to 0 to disable, so that only real HTTP/3 errors demote an origin.

Default: 3.
```

- [ ] **Step 7: Regenerate `index.d.ts` and check the type landed**

Run: `npm run build && grep -n "upgradeCancelStrikes" index.d.ts`

Expected: one match, `upgradeCancelStrikes?: number`. `index.d.ts` is generated by napi — never hand-edit it.

- [ ] **Step 8: Run the full suite for regressions**

Run: `cargo test && npm run test:only`

Expected: all pass. `npm run test:only` needs httpbin — start it with
`go run github.com/mccutchen/go-httpbin/v2/cmd/go-httpbin@v2.23.1 -host 127.0.0.1 -port 8888 &`
and set `HTTPBIN_URL=http://localhost:8888`.

- [ ] **Step 9: Commit**

```bash
jj describe -m "fix: fall back to TCP when a cancelled h3 attempt hides the failure

AltSvcMiddleware could only demote an origin from the h3 attempt's return
value, but faith_fetch races send() against the abort signal in a select!,
which drops that future. record_h3_failure was therefore unreachable under
cancellation: the confirmed Alt-Svc entry survived its 24h TTL and every
retry re-attempted h3 over a dead UDP path while TCP was healthy.

A drop guard around the attempt records a cancellation strike, so a
sustained run of cancelled attempts demotes the origin. Exposes the
threshold as http3.upgradeCancelStrikes (default 3, 0 disables).

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj new
```

---

### Task 3: Attempt deadline

An optional ceiling on time-to-response-headers for the h3 attempt, so a hang becomes a normal failure-and-fallback without depending on caller behaviour at all.

**Files:**
- Modify: `src/alt_svc.rs` (`AltSvcMiddleware` struct 214-218, `new` 229-238, `Debug` 220-227, the `trying_h3` branch of `handle`)
- Modify: `src/agent.rs` (`AgentHttp3Options`, and the `client.with(AltSvcMiddleware::new(...))` call at ~713)
- Modify: `README.md`, `index.d.ts` (regenerated)
- Test: `test/http3-abort-fallback.test.js` (add a second test)

**Interfaces:**
- Consumes: `H3AttemptGuard::new(Arc<AltSvcCache>, reqwest::Url)` and `H3AttemptGuard::disarm(&mut self)` from Task 2.
- Produces: `AltSvcMiddleware::new(cache: Arc<AltSvcCache>, enabled: bool, attempt_timeout: Option<Duration>) -> Self`; `AgentHttp3Options.upgrade_attempt_timeout: Option<u32>` (JS: `upgradeAttemptTimeout`).

- [ ] **Step 1: Write the failing test**

Add to the end of `test/http3-abort-fallback.test.js`. It sets `upgradeCancelStrikes: 0` so only the deadline can produce the fallback, and uses a patient caller with no `AbortSignal` so cancellation plays no part:

```javascript
test("HTTP/3: upgradeAttemptTimeout falls back without any cancellation", async (t) => {
	if (!SUPPORTED) {
		t.pass(`skipped on ${process.platform} (linux-only harness)`);
		t.end();
		return;
	}
	if (!caddyAvailable()) {
		if (process.env.CI) {
			t.fail("caddy is not on PATH but CI is set; the install step must provide it");
			t.end();
			return;
		}
		t.pass("skipped: caddy not on PATH (install it to run this test locally)");
		t.end();
		return;
	}

	const { Agent } = require("../index.js");
	const { fetch } = require("../wrapper.js");
	const { ca } = ensureCert();

	const front = await findFreePort();
	const back = await findFreePort();
	const caddy = await startCaddy({ port: back, dir: os.tmpdir() });
	const tcp = await startTcpProxy({ listenPort: front, upstreamPort: back });
	const relay = await startUdpRelay({ listenPort: front, upstreamPort: back });

	const agent = new Agent({
		tls: { extraRoots: [ca] },
		http3: {
			hints: [{ host: "localhost", port: front }],
			// Low enough to prove the deadline fires well before quinn's 30s idle
			// timeout, but with enough headroom for the warm-up QUIC handshake on a
			// loaded CI runner (~90ms locally).
			upgradeAttemptTimeout: 1500,
			// Isolate the deadline: strikes must play no part in this fallback.
			upgradeCancelStrikes: 0,
		},
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});
	const url = `https://localhost:${front}/`;

	const attempt = async (opts) => {
		try {
			const res = await fetch(url, { agent, ...opts });
			await res.text();
			return { ok: true, version: res.version };
		} catch (err) {
			return { ok: false, code: err.code };
		}
	};

	try {
		let warm;
		for (let i = 0; i < 3; i++) warm = await attempt({ timeout: 10000 });
		t.equal(
			warm.version,
			"HTTP/3.0",
			"precondition: the origin is confirmed as HTTP/3 through the relay",
		);

		relay.blackhole();

		// Patient caller, no abort signal: the middleware's own deadline is the
		// only thing that can end this attempt.
		const first = await attempt({ timeout: 15000 });
		t.ok(first.ok, "the very first request after the break already falls back");
		t.equal(first.version, "HTTP/2.0", "the fallback used TCP");
	} finally {
		relay.close();
		await tcp.close();
		caddy.close();
		t.end();
	}
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `npm run build && npx tape test/http3-abort-fallback.test.js`

Expected: the first test passes 3/3; the new test FAILS. `upgradeAttemptTimeout` is not a real option yet, so the h3 attempt hangs until quinn's 30s idle timeout, which exceeds the caller's 15s `timeout` — so `first.ok` is false.

- [ ] **Step 3: Add the field to the middleware**

In `src/alt_svc.rs`:

```rust
#[derive(Clone)]
pub struct AltSvcMiddleware {
	cache: Arc<AltSvcCache>,
	enabled: bool,
	/// Ceiling on how long an HTTP/3 attempt may take to produce response
	/// headers before it is treated as failed and retried over TCP.
	attempt_timeout: Option<Duration>,
}

impl std::fmt::Debug for AltSvcMiddleware {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AltSvcMiddleware")
			.field("enabled", &self.enabled)
			.field("attempt_timeout", &self.attempt_timeout)
			.field("cache", &self.cache)
			.finish()
	}
}

impl AltSvcMiddleware {
	pub fn new(cache: Arc<AltSvcCache>, enabled: bool, attempt_timeout: Option<Duration>) -> Self {
		Self {
			cache,
			enabled,
			attempt_timeout,
		}
	}

	#[allow(dead_code)]
	pub fn cache(&self) -> &Arc<AltSvcCache> {
		&self.cache
	}
}
```

- [ ] **Step 4: Apply the deadline in `handle`**

Replace the guard/attempt/match block from Task 2 with this. The outcome **must** be bound in its own statement: if `tokio::time::timeout(...)` appears directly as a `match` scrutinee, the temporary holding the mutable borrow of `extensions` lives until the end of the `match`, and the fallback arm needs `extensions` — which fails to borrow-check.

```rust
				let mut guard = H3AttemptGuard::new(Arc::clone(&self.cache), url.clone());
				// `None` means the attempt ran out of time. Bound in its own
				// statement so the mutable borrow of `extensions` ends here,
				// leaving the fallback below free to use it.
				let outcome = match self.attempt_timeout {
					Some(limit) => tokio::time::timeout(limit, next.clone().run(req, extensions))
						.await
						.ok(),
					None => Some(next.clone().run(req, extensions).await),
				};
				// Reached on success, error and expiry alike; only a mid-flight
				// drop skips it and leaves the guard armed.
				guard.disarm();

				match outcome {
					Some(Ok(response)) => {
						if response.version() == http::Version::HTTP_3 {
							self.cache.confirm_h3(&url);
						}

						if let Some(alt_svc) = response.headers().get("alt-svc") {
							if let Ok(value) = alt_svc.to_str() {
								if let Some((port, max_age)) = parse_alt_svc_header(value) {
									self.cache.record_alt_svc(&url, port, max_age);
								}
							}
						}

						Ok(response)
					}
					// An expired deadline is as good as an error: HTTP/3 did not
					// deliver. Taking the fallback branch directly avoids having
					// to synthesise a reqwest_middleware::Error, which would mean
					// adding anyhow as a dependency.
					Some(Err(_)) | None => {
						self.cache.record_h3_failure(&url);

						// Use the cloned request (which still has default HTTP version)
						next.run(req_clone, extensions).await
					}
				}
```

- [ ] **Step 5: Expose `upgradeAttemptTimeout` and wire it up**

In `src/agent.rs`, add to `AgentHttp3Options` after `upgrade_cancel_strikes`:

```rust
	/// How long to wait for HTTP/3 response headers before giving up on the HTTP/3
	/// attempt and retrying the request over TCP, in **milliseconds**.
	///
	/// Note the unit: the other `upgrade*` settings are in seconds, but this one is
	/// in milliseconds to match the `timeout` settings, because useful values are
	/// sub-second.
	///
	/// This only bounds the wait for response headers, not the response body, so a
	/// slow body is unaffected. The default is deliberately high enough never to
	/// trip in normal operation: HTTP/3's own idle timeout (`maxIdleTimeout`,
	/// default 30 seconds) fires first and reports a proper error. Lower it if you
	/// want faster recovery when a UDP path breaks.
	///
	/// Set to 0 to disable, so an HTTP/3 attempt is bounded only by the QUIC idle
	/// timeout and the request's own timeout.
	///
	/// Default: 60000 (60 seconds).
	pub upgrade_attempt_timeout: Option<u32>,
```

Then, in the same `#[cfg(feature = "http3")]` block that reads the other options, derive the `Duration` (0 means disabled):

```rust
			let attempt_timeout = match http3_opts
				.and_then(|o| o.upgrade_attempt_timeout)
				.unwrap_or(60_000)
			{
				0 => None,
				millis => Some(Duration::from_millis(millis.into())),
			};
```

And pass it when registering the middleware, replacing the existing `client.with(AltSvcMiddleware::new(cache.clone(), enabled));`:

```rust
			client = client.with(AltSvcMiddleware::new(
				cache.clone(),
				enabled,
				attempt_timeout,
			));
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `npm run build && npx tape test/http3-abort-fallback.test.js`

Expected: PASS, 6/6 across both tests.

- [ ] **Step 7: Document it in the README**

In `README.md`, add immediately after the `upgradeCancelStrikes` section from Task 2:

```markdown
#### `AgentOptions.http3.upgradeAttemptTimeout: number`

How long to wait for HTTP/3 response headers before giving up on the HTTP/3 attempt and retrying
the request over TCP, in **milliseconds**. Note the unit: the other `upgrade*` settings are in
seconds, but this one matches the `timeout` settings, because useful values here are sub-second.

This bounds only the wait for response headers, not the response body, so slow bodies are
unaffected. The default is deliberately high enough that it never trips in normal operation —
HTTP/3's own idle timeout (`maxIdleTimeout`, 30 seconds by default) fires first and reports a
proper error. Lower it if you want faster recovery when a UDP path breaks: a value of a few
hundred milliseconds makes a broken HTTP/3 path fall back to TCP within the first request, rather
than after `upgradeCancelStrikes` cancelled ones.

Set to 0 to disable.

Default: 60000 (60 seconds).
```

- [ ] **Step 8: Regenerate `index.d.ts` and run the full suite**

Run: `npm run build && grep -n "upgradeAttemptTimeout" index.d.ts && cargo test && npm run test:only`

Expected: one `index.d.ts` match (`upgradeAttemptTimeout?: number`), and all tests pass. httpbin must be running as in Task 2 Step 8.

- [ ] **Step 9: Commit and move the bookmark**

```bash
jj describe -m "feat: bound the HTTP/3 attempt with http3.upgradeAttemptTimeout

Gives HTTP/3 attempts a ceiling on time-to-response-headers, after which the
request is retried over TCP. Unlike cancellation strikes this needs no help
from the caller, so a broken UDP path can fall back within a single request.

Defaults to 60s, above the 30s QUIC idle timeout, so it stays inert unless
maxIdleTimeout is raised or it is deliberately lowered for fast recovery.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj bookmark set claude/h3-cancellation-fix -r @
jj new
```

---

## Verification before opening the PR

- [ ] `cargo test` passes.
- [ ] `npm run test:only` passes with httpbin running, including both tests in `test/http3-abort-fallback.test.js`.
- [ ] `git status`/`jj status` shows `index.d.ts` regenerated rather than hand-edited.
- [ ] `src/fetch.rs` is untouched (`jj diff --stat` must not list it).
- [ ] No new entries in `Cargo.toml` `[dependencies]`.
- [ ] PR #23's test is passing on this branch, which is the acceptance criterion for the whole plan.

## Acceptance criteria (from the spec)

1. With UDP blackholed and a caller cancelling via `AbortSignal`, a retry loop falls back to TCP within `upgrade_cancel_strikes + 1` attempts.
2. An origin whose h3 works normally is never demoted by interleaved caller aborts.
3. A low `upgradeAttemptTimeout` (1500ms in the test) causes fallback on the first attempt, with `upgradeCancelStrikes: 0` proving strikes played no part.
4. Default behaviour for healthy origins is unchanged: h3 is still preferred and confirmed.
