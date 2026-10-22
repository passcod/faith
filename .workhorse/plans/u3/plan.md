# Read CRLite data from a system upki cache

Add upki as a second provider of CRLite filters, preferred where its cache is usable and fresh, with Faith's own signed fetch as the fallback.
The query side already takes a set of decoded filters from whoever supplied them (`crlite::Filters`), so this is a new loader, a second held-data slot in the store with a per-agent choice between the two, and a poller that also watches the upki manifest locally.
Specs: the system's CRLite data section of [REVOC](../../specs/agent/revocation.md#the-systems-crlite-data), with folds into TLS, OBS, ENV and RUST.

## Design notes

### What upki writes, and what Faith reads

Confirmed against upki's source and this machine's Ubuntu-style cache (`/etc/upki/config.toml`, `/var/cache/upki/revocation`).

- The CRLite data is `<cache-dir>/revocation/{index.bin,manifest.json,*.filter,*.filter.delta}`.
- `index.bin` is the authority on which filter files exist; its filename table (32-byte NULL-padded slots, `upkiidx1`, or `upkiidx0` with a `u8` count) is all Faith needs, because Faith loads every named filter and queries each, and a filter answers `NotCovered` for timestamps outside its own coverage. The log directory and entry sections are an index for upki's lazy loading and are not read.
- `manifest.json` is upki's own: `generated_at`, `comment`, `files[{filename,size,hash}]` with a hex SHA-256. Every successful `upki fetch` rewrites it, even on a 304, so its mtime is the last confirmation.
- Write order in upki is filters, then `index.bin`, then `manifest.json`, then deletion of unreferenced files, each atomically. A reader that sees a named filter missing has raced an update and re-reads from the start.
- Filter names are `[A-Za-z0-9-._]`, 1 to 32 bytes, never `.` or `..`; enforce before joining a name onto the directory.
- upki locates its files with `ProjectDirs::from("dev", "rustls", "upki")` (the `directories` crate, v6, the version `web-faith` already uses), then `/etc/upki`, `/Library/Application Support/upki`, or `%ProgramData%\rustls\upki`; the cache directory falls back to the per-user one if it exists, then the system one (`/var/cache/upki`, `/Library/Caches/dev.rustls.upki`, `%ProgramData%\rustls\upki\cache`).
- This machine's timer is 6 hourly, the card's "about two hours" being upki's packaging suggestion.

### Tests cannot set environment variables

The workspace rule is no `std::env::set_var`, so location lookup takes its directories as a value (`Locator`) that `Locator::platform()` fills from `directories` and the platform constants, and tests fill with temp directories.
`%ProgramData%` is only read, never set.

### Store: two CRLite slots and a per-agent choice

`Store.crlite` stays Faith's own Remote Settings data.
A new `Store.upki` slot holds the upki data: the decoded `Filters`, the set of filter hashes, the manifest mtime read from the open file handle (so it describes the bytes read), and the confirmed time.
It does not reuse `Held<T>`, which is built around a signed changeset.

The choice for a handshake is a function of the agent's `upki` flag and three facts: upki data held, whether it is fresh, and each provider's confirmed time:

- agent has upki off: own data.
- upki held and fresh: upki.
- upki held and stale: whichever of the two was confirmed more recently, upki if own is absent.
- no upki data: own data.

Freshness is a flag the loader and the poller's local check maintain rather than a clock read on every handshake.
Whenever the flag flips, or a reload changes the filter set, the epoch moves so saved sessions are not resumed; a reload whose filter set is unchanged only updates the confirmed time.

### What the poller does

Today the poller wakes on a daily timer and sends one request listing every followed collection.
It gains a second timer for the local upki check, about every ten minutes, which stats the manifest, reloads when its mtime changed, and re-evaluates freshness.
CRLite is left out of the Remote Settings check, and out of `next_due`, unless some agent needs Faith's own data: an agent has upki off, or upki is not usable and fresh.
When the local check finds that demand has appeared, the Remote Settings check is brought forward to now.
The poller thread is still started when only CRLite-via-upki is in use, since the local check needs it.

Faith's own CRLite cache is loaded lazily: at agent construction only if the first agent needs own data (upki off, absent or stale), otherwise on the poller thread when demand first appears, guarded by the same `Once` either way.
That keeps a 6 MB decode off the startup of a host whose upki cache is fresh.

### Public API of `web-faith-revocation`

The crate has no release tag, so signatures can still change; check `git tag` and crates.io before landing, and keep the old `enable(source)` shape if it turns out to be published.

- `RevocationConfig` gains `upki: UpkiLocation` (`System` by default, `Dir(PathBuf)` to name the `revocation` directory directly, for tests and embedders), `upki_poll_interval` (ten minutes) and `upki_max_age` (three days). These are config fields with defaults so tests need not wait; the spec's numbers are the defaults.
- `Sources` gains `upki: bool`, on in `all()`, behind the `upki` feature.
- `Revocation::enable` takes the agent's `Sources`, since the per-source loop in `RevocationRecipe::start` loses the information it needs.
- `Revocation::in_force(source, upki) -> Option<InForce>` replaces `last_confirmed`; `InForce { provider: Provider, confirmed: Option<SystemTime> }` and `Provider::{RemoteSettings, Upki}`, both `#[non_exhaustive]`.

### Real-data check

Alongside the builder-made fixtures, load this machine's real cache and compare verdicts with `upki revocation` for a handful of public sites' chains, as T3 did against the live filter. Not committed: the cache is too large for a fixture, and it confirms that the real encoding, names and hashes parse. An `#[ignore]`d test that reads the system location is reasonable if it needs no network.

## Checklist

### 1. Locate the cache (`upki/locate.rs`)

- [ ] `upki` feature on `web-faith-revocation`: `upki = ["crlite", "dep:toml", "dep:directories"]`, both added with `cargo add` (`toml` with only `parse` and `serde`)
- [ ] `Locator { user_config_dir, user_cache_dir, system_config_dir, system_cache_dir }` and `Locator::platform()` from `directories::ProjectDirs::from("dev", "rustls", "upki")` plus the per-platform system constants, with `%ProgramData%` read and defaulted to `C:\ProgramData`
- [ ] Config choice: user `config.toml` if it exists, else the system one, else none; a file that exists but cannot be read, or has no `cache-dir`, leaves the cache unlocated; unknown keys are ignored
- [ ] No config file: the user cache directory if it exists, else the system one if it exists, else unlocated
- [ ] Resolve `<cache-dir>/revocation` from the `cache-dir` key
- [ ] `UpkiLocation::{System, Dir}` on `RevocationConfig`; `Dir` skips the lookup
- [ ] Unit tests with temp directories: user config beats system, config `cache-dir` honoured, no config with a user cache, no config with only a system cache, nothing present, malformed config, config without `cache-dir`

### 2. Read the cache (`upki/read.rs`, `upki/index.rs`)

- [ ] `index.rs`: parse `upkiidx1` and `upkiidx0` headers and the filename table from one read of `index.bin`, bounds-checking every count against the file length, rejecting a table that does not fit, and validating each name (`[A-Za-z0-9-._]`, 1 to 32 bytes, not `.` or `..`, no path separators); capped file size
- [ ] `read.rs`: open `manifest.json`, take the mtime from the same handle, then parse it; read `index.bin`; for each named filter require a manifest entry, read the file (capped like `cache::MAX_FILE`), check its SHA-256 against the manifest, decode with `Filter::decode`; an index naming no filters is unusable
- [ ] A missing filter re-reads from `index.bin` once before the cache counts as unusable; any other failure is unusable at once
- [ ] Reuse already-decoded filters by hash on a reload, so a 6 hourly rewrite of the manifest costs two small reads rather than a 6 MB decode
- [ ] Result type carrying the `Filters`, the hash set, and the confirmed time; errors fold into the crate's `Error` with a variant for a damaged cache
- [ ] Unit tests on bytes: both index versions, bad magic, truncated header, a name count beyond the file, a log count that overflows, names with `..`, `/`, `\`, NUL inside, 33 bytes, empty, and a table that ends mid-name
- [ ] `test_support`: a writer that lays down a upki-format directory (index with an empty log directory, manifest, filters from `crlite_filter`) so tests can also corrupt a filter, drop a file, drop the manifest, change an mtime with `File::set_modified`, and swap in a new set
- [ ] Unit tests on the fixture: usable cache, filter missing, filter hash mismatch, filter not in the manifest, no manifest, index naming nothing, undecodable filter, a name that would escape the directory

### 3. Store: second slot and per-agent choice (`store.rs`)

- [ ] `Store.upki: RwLock<UpkiHeld>` and a `upki_fresh` flag; `upki_poll`-facing methods `load_upki` (once, at first enable), `refresh_upki` (stat the manifest, reload on a changed mtime, re-evaluate freshness, return what changed) and `wants_own_crlite`
- [ ] `refresh_upki` re-runs the lookup every call, so a cache that appears, moves, or vanishes is noticed; a failed reload keeps the data held; an unchanged filter set updates only `confirmed`; a changed set or a flipped freshness bumps the epoch
- [ ] Selection function per the design notes, used by `refusing_source` in place of reading `self.crlite` directly; `refusing_source` takes the agent's `Sources.upki`
- [ ] Lazy own-cache load: split `load(Source::Crlite)` so the `Once` can be fired from `enable` or from the poller
- [ ] `in_force(source, upki)` and `has_data` updated for the two slots; `held`/`confirmed` keep meaning Faith's own data
- [ ] `// spec:REVOC#a-usable-cache`, `#a-fresh-cache`, `#choosing-between-providers` traceability comments
- [ ] Store tests over the matrix: fresh upki beats own; upki absent gives own; stale upki and no own data stays in force; stale with newer own gives own; stale with older own gives upki; agent with upki off gets own only; freshness flipping both ways moves the epoch; an unchanged reload does not; a failed reload keeps the data held

### 4. Poller (`poller.rs`)

- [ ] Message type for the channel carries the agent's `Sources` so the poller learns whether any agent wants own CRLite
- [ ] Second timer for the local check at `upki_poll_interval`; a tick calls `refresh_upki`, and brings the Remote Settings check forward when own demand has appeared
- [ ] `poll` and `next_due` leave CRLite out while `wants_own_crlite` is false; `poll` returns `true` when only CRLite was skipped
- [ ] Fire the lazy own-cache load on the poller thread when demand first appears
- [ ] The poller thread still starts for CRLite-via-upki alone
- [ ] Tests against the fake server with a fixture directory: fresh upki means no CRLite request while OneCRL is still fetched; ageing the manifest past `upki_max_age` starts CRLite requests; refreshing it stops them; an agent with upki off fetches regardless; a cache that appears mid-process is picked up; a manifest rewrite with new filters is enforced without a restart

### 5. Crate surface and docs (`lib.rs`, README)

- [ ] `RevocationConfig` fields, `Sources.upki`, `Provider`, `InForce`, `Revocation::enable(Sources)` and `in_force`, all behind `upki` where they only mean something with it; `mozilla()` defaults
- [ ] Handshake tests through real rustls: a leaf revoked in upki data is refused naming CRLite; a leaf revoked only in own data passes an agent on fresh upki
- [ ] Crate README and `lib.rs` feature tables and module docs: the `upki` feature, the trust model in a sentence, the example

### 6. `web-faith` integration

- [ ] Feature `upki = ["crlite", "web-faith-revocation/upki"]`, default; `lib.rs` and README feature tables
- [ ] `RevocationOptions.upki: Option<bool>` behind the feature, defaulting to on, with a `compile_fail` doctest when the feature is out, as the other two have
- [ ] `revocation_sources` in `agent/build.rs` sets `Sources.upki`; `RevocationRecipe::start` passes the whole `Sources` to `enable`
- [ ] `RevocationStats` gains `provider: Option<RevocationProvider>`, set for CRLite and `None` for OneCRL, and `last_confirmed` comes from `in_force`; update the doc comment that says the time is the same for every agent
- [ ] `install` (the `unstable-internals` hook) already takes a `RevocationConfig`, so tests point it at a fixture directory with `UpkiLocation::Dir`
- [ ] `tests/revocation.rs`: an agent on a fixture upki cache refuses a revoked leaf and reports `upki` with the manifest's mtime; a second agent with `upki: false` is served from the fake Remote Settings server and reports `remote-settings`; a stale fixture reports `remote-settings` once the fetch lands
- [ ] Builds: `--no-default-features --features tls-aws-lc-rs,crlite` (no `upki`), `...,onecrl` only, and the default set

### 7. Node surface

- [ ] `web-faith-napi` feature `upki = ["web-faith/upki"]`, in its default set
- [ ] `AgentRevocationOptions.upki?: boolean` in `agent/options.rs`, mapped in `agent/convert.rs`
- [ ] napi `RevocationStats` gains `provider: 'remote-settings' | 'upki' | null`; rebuild to regenerate `index.d.ts`; re-export any new type from `wrapper.d.ts`
- [ ] Tape tests: the option is accepted, the stats shape carries `provider` for CRLite and `null` for OneCRL

### 8. Docs and checks

- [ ] Root README: `AgentOptions.tls.revocation.upki`, the `provider` stats field, how upki is found and when it is preferred, the revocation cache section pointing at upki's XDG variables, and the container note that a mounted host cache needs no download
- [ ] Real-data check against this machine's cache as described in the design notes
- [ ] `cargo fmt`, `cargo clippy` across the feature sets above, `cargo test`, and the Node suite with `HTTPBIN_URL=http://localhost:8888` after `NODE_ENV=development npm run build`
- [ ] Confirm CI builds `web-faith-revocation` on macOS and Windows with the `upki` feature, since the platform constants are cfg-gated there and nothing locally compiles them
