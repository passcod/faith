# Certificate revocation checking

Check server certificates against Mozilla's OneCRL (intermediates) and CRLite (leaves), fetched and kept current at runtime, each on by default with its own opt-out.
Faith hands reqwest a rustls config of its own whose verifier wraps the platform verifier, rather than replacing reqwest's TLS stack.

## Findings

### Where reqwest lets us in

reqwest 0.13 has no hook for a custom certificate verifier.
Its own CRL support (`tls_crls_only`) only works with `tls_certs_only`, i.e. it drops the platform verifier, which Faith's trust model ([TLS](../../specs/agent/tls.md)) rules out.

The one way in is `tls_backend_preconfigured(rustls::ClientConfig)`.
That hands reqwest a finished config, and reqwest then skips everything it would otherwise build into it, so Faith takes over:

- the verifier (platform verifier with extra roots, or the no-verify one for `NODE_TLS_REJECT_UNAUTHORIZED=0`)
- client identity (mTLS)
- `SSLKEYLOGFILE` key logging
- TCP ALPN (`h2`, `http/1.1`): reqwest sets none on a preconfigured config
- HTTP/3 early data: reqwest clones the config for QUIC and sets `h3` ALPN, but no longer applies `tls_early_data`
- the crypto provider choice (aws-lc-rs vs ring)

What stays with reqwest: connector setup, `tls_info` (peer certificate capture), `https_only`, the HTTP/3 connector.
This is a few dozen lines replacing the `tls_*` builder calls in `ClientRecipe::build`, not a TLS stack rewrite.
The cost is that the preconfigured path is documented as semver-unstable: the `rustls` version must match reqwest's exactly or the build fails at runtime with "unknown TLS backend".

### The verifier

A wrapping `ServerCertVerifier`: delegate to the platform verifier, and on success run the revocation check.
Signature checks and scheme lists pass straight through.

On Linux the platform verifier does no revocation checking at all; on macOS and Windows the OS does its own (OCSP/CRL), and ours runs on top.

### Candidate data sources

| | OneCRL | CRLite (via upki) |
|---|---|---|
| Covers | intermediates (and a few roots) | end-entity certificates |
| Publisher | Mozilla Remote Settings | rustls project mirror of Mozilla's CRLite |
| Size | ~1900 entries, ~800 KB JSON | ~7.5 MB (full filter plus deltas) |
| Churn | rare | about twice daily |
| Integrity | content signature (ECDSA P-384, x5u chain) | SHA-256 hashes in a manifest |
| Rust support | none, we'd parse and match ourselves | `upki` / `rustls-upki` 1.0 |

OneCRL entries are mostly issuer DN plus serial, a handful subject DN plus SPKI hash.
Matching against only the certificates the server presented can wrongly reject a connection where a revoked cross-sign is presented but a valid path exists.
Doing it right means checking the built path; webpki's `verify_for_usage` takes a `verify_path` callback that rejects a candidate path and lets path building try the next, but only where webpki is the path builder (Linux), not where the OS builds the path.

`rustls-upki`'s verifier verifies with webpki against a fixed root store, so it can't be used as-is with the platform verifier.
The lower-level `upki::revocation` API (`RevocationCheckInput`, `Store::check`) can be called from our wrapper.
It pulls `rustls-webpki` 0.104 (alpha) alongside our 0.103, and reads data from a cache directory rather than memory.

### CRLite straight from Remote Settings

Mozilla publishes the CRLite filters in Remote Settings too (`security-state/cert-revocations`), as attachments on records signed by the same signer as OneCRL.
Each record carries its attachment's SHA-256, so the content signature covers the filter bytes as well.
upki's mirror serves these same files; upki adds an unsigned manifest and its own index over them.

The default channel is one full filter (~6 MB, the current one dated 18 Oct) plus a delta chain of ~250 KB twice a day, each delta naming its parent.
Filters are clubcard, queried with Mozilla's `clubcard-crlite` (MPL-2.0, which upki also depends on).

A query needs the leaf's serial, the issuer's SPKI hash and the leaf's SCT timestamps.
webpki 0.103 doesn't expose SCTs, so we'd parse the SCT list extension ourselves.
A leaf with no SCTs (private CAs, extra roots) or logged after the filter's coverage comes back as not covered, which is a pass.
Stale data loses coverage, but it can't produce a false rejection.

Going through upki instead: its fetch builds its own reqwest client, ignoring the agent's proxy, DNS and extra roots, and its cache location only comes from a TOML config file.

## Decisions

### OneCRL and CRLite, both from Remote Settings

Two sources, each checked inside the same wrapping verifier:

- OneCRL (`security-state/onecrl`) for intermediates
- CRLite (`security-state/cert-revocations`, default channel) for leaf certificates

Both come straight from Mozilla Remote Settings, not through upki, so they share one client: change check, content-signature verification, disk cache, poller.
CRLite adds attachment download with hash checks against the signed records, full-plus-delta chain assembly, `clubcard-crlite` queries, and SCT extraction from the leaf.

The point is that a revocation takes effect at runtime, without a Faith release.
The eMudhra distrust needed a `rustls-webpki` bump to ship, and that same root is OneCRL's newest entry.

### Separate opt-outs, as agent options only

`tls.revocation.onecrl` and `tls.revocation.crlite`, booleans defaulting to true, mirrored on the Rust `TlsOptions` builder.
CRLite is the heavy one (a ~6 MB first download, ~500 KB of deltas a day, ~6 to 7 MB resident), so callers can keep OneCRL while dropping it.
An agent with a source off skips that check; the source's poller starts with the first agent that uses it.

No environment variable.
ENV keeps to Node's vocabulary plus the standard proxy and OpenSSL variables, and a `FAITH_*` kill switch would be the first of its kind.
`NODE_TLS_REJECT_UNAUTHORIZED=0` already turns revocation off along with the rest of verification, and a caller wanting a targeted switch can wire their own variable into the option.
One can be added later without breaking anyone; removing one couldn't.

### Background polling, one process-wide set

Faith polls Remote Settings itself, using the cheap change check, at roughly Firefox's daily cadence.
The set and the poller are process-wide and shared by every agent, so many agents in one process cost one fetch.
Polling only runs for sources some agent in the process has left on.

### On-disk cache for cold start

Each verified set is written to a cache directory and loaded when the first agent using that source is constructed, so a restart enforces the last known set from its first connection.
Loading at construction rather than at the first handshake means a handshake never waits on the disk and never races the load; the cost is a few MB read and verified synchronously, once per process.
Before any set is known (a fresh machine, or an unwritable cache), nothing is enforced until the first fetch lands.

No compile-time snapshot: webpki's own distrust releases already cover the window it would, so embedding one adds nothing.

### Content signatures are verified

Fetched data is checked against its Remote Settings content signature before use, as Firefox does: canonical JSON of the records, `x5u` chain to a pinned Mozilla content-signing root, SAN `onecrl.content-signature.mozilla.org` (which signs both collections), P-384 ECDSA.
The disk cache keeps the signature alongside the records and is re-verified on load.
This stops someone who can intercept TLS to Mozilla from stripping entries, which is the same attacker a revoked intermediate helps.

### Path linking over presented certificates and a trust-store index

The platform verifier says a trusted path exists but not which one, servers often send stale cross-signs alongside it, and they rarely send the root.
So from the leaf, the check links each certificate to every candidate that could have issued it (subject matches issuer, and SKI matches AKI where both exist), and repeats upwards.
Candidates are the presented certificates plus the agent's trust-store roots.
A connection is rejected only when every path built this way hits a OneCRL entry.
Issuer-and-serial entries match on those fields, subject-and-key-hash entries on theirs.

The platform roots come from `rustls-native-certs`, which lists the store on all three platforms and is what the platform verifier itself uses on Linux.
A process-wide index of them (subject and SKI to certificate) is rebuilt off the handshake path after each OneCRL load or update, so a handshake only does map lookups; before the first build, the root step is skipped and fails open.
The agent's extra roots and `NODE_EXTRA_CA_CERTS` are added per agent, since they are per-agent.
Windows roots the OS fetches on demand from Windows Update aren't in the local store, and AIA-fetched intermediates aren't seen at all; both fail open.

Linkage is by name and key ID, not signatures: the platform verifier has already proved a valid path, this only works out which certificates could be on it.
It behaves the same on every platform.

CRLite uses the same linking to find the leaf's issuer, whose SPKI hash the query needs.
With several candidate issuer keys the leaf is refused only when revoked under all of them; in practice there is one.
`clubcard-crlite` answers `NotEnrolled` for an unknown issuer and `NotCovered` for timestamps outside the filter, and both pass.

### CRLite filters are queried independently

`clubcard-crlite` deltas aren't patches on the full filter: each is a filter of its own covering its window, and a leaf is revoked if any held filter says so.
So a missing delta only loses its window's revocations and can never cause a false rejection; every held filter that verifies is used.
A new full filter replaces the old full filter and its deltas once downloaded and verified.

### Rollback is refused

The signature covers the collection's timestamp, so data signed earlier than what is held is rejected, as Firefox does.
Without this, an interceptor could replay an older, validly signed collection from before an entry was added.
A process with nothing held yet can still be fed an old collection; that's unavoidable.

### Session resumption is cut on data change

rustls skips the verifier on resumption, and the stored chain in a session value is `pub(crate)`, so a session store can't re-check it.
Instead, a process-wide epoch counter bumps whenever a source's data changes, and a wrapping `ClientSessionStore` drops tickets stored under an older epoch.
reqwest's HTTP/3 config is a clone sharing the same store `Arc`, so QUIC is covered too.
Cost: one full handshake per origin after each update (CRLite updates about twice a day); 0-RTT resumes from the next ticket.

Open connections are left alone: an update applies to connections made after it, and pooled ones lapse on the idle window.

### Staleness and errors

Held data never expires; `stats()` shows its age.
A refusal returns rustls's `CertificateError::Revoked` wrapped with the source, so the `Network` error's message names revocation and which source refused.

### Cache pruning

After each verified update, filter files no current signed record names are deleted once past a grace period, so another process still on the older set isn't cut off.
On Unix an open file survives unlinking; on Windows the delete fails and is retried next update.

### A component crate: `web-faith-revocation`

It holds the wrapping `ServerCertVerifier`, the Remote Settings client, the OneCRL set and the CRLite store.
Like `web-faith-alt-svc` it takes a `reqwest::Client` to fetch with, and it has its own error type.
It is useful on its own: nothing else gives a rustls user OneCRL and CRLite on top of the platform verifier.

Cargo features `onecrl` and `crlite`, passed through by `web-faith` and both on by default, so a Rust build can drop CRLite and its MPL-2.0 `clubcard-crlite` dependency at compile time.
The RUST spec gains a sixth component crate; its "TLS isn't a component crate" line still holds, as this is a verifier plugin rather than the TLS stack.

The pinned signing root and the Remote Settings base URL are parameters of the crate's API, so tests can stand up a fake server signed by a test root.
`web-faith` always passes Mozilla's.

### Fetching and polling

The poller fetches with its own internal client, built from the default recipe and the environment, not borrowed from whichever agent came first.
Proxies only come from the environment and the OS, so it sees the same proxy configuration any agent would; `NODE_EXTRA_CA_CERTS` and `NODE_USE_ENV_PROXY` apply too, and agent options never do.
It reaches `firefox.settings.services.mozilla.com` and the attachments host that service names, and moves no agent's counters.

The spec only requires it to last as long as the process without keeping it alive; how is a choice made here.
Other ways would do: a plain thread with a blocking loop, or a task on a runtime Faith keeps for itself.
Chosen: it runs on a dedicated thread with its own current-thread Tokio runtime, spawned once per process at the first agent construction that uses a source, after the cache load.
`web-faith` owns no runtime on the Rust surface (its other background work borrows the caller's), and a poller tied to a caller's runtime would stop with it; Node uses the same thread rather than napi's `tokio_rt` so there is one code path.
The thread is detached, so it doesn't hold up a Rust `main` returning, and a native thread doesn't hold Node's event loop open.
A source an agent turns on later is handed to the running thread; nothing is fetched for a source no agent uses.
It won't fetch straight away when the cache was confirmed within the polling interval, so constructing an agent on a warm host costs no request.
The disk cache is read during agent construction (see above), never inside a handshake.

A failed fetch, bad signature or hash mismatch keeps the last good set and retries with backoff.
Revocation data only ever adds distrust, so failure leaves the connection to the platform verifier alone.

### Signature crypto from the rustls provider

P-384 ECDSA verification uses the rustls crypto provider already chosen (aws-lc-rs or ring), so no new crypto dependency.
Mozilla's signatures are raw `r‖s` and are converted to DER first.

### Cache in the platform's user cache directory

The cache is the platform user cache directory from `directories` (`~/.cache/web-faith/revocation` on Linux, the `~/Library/Caches` and `%LOCALAPPDATA%` equivalents elsewhere).
On Linux that honours `XDG_CACHE_HOME`, which gives container operators a way to point it at a volume through a platform variable; ENV mentions it.
There is no Faith option for it, since a per-agent setting on process-wide state would need a first-agent-wins rule.
The component crate takes the path explicitly, so a Rust caller embedding it directly chooses its own.

CRLite filter files are stored named by their hash, so processes on one host share a download, and every read is checked against the signed record anyway.
Writes go through a temporary file and an atomic rename.
Each process still polls on its own, which is a small conditional request.

Where the directory is missing or unwritable, the set is held in memory only.

### Reporting through `stats()`

Each agent's `stats()` gains, per source it uses, a count of its connections that source refused and when the data was last confirmed current.
A refusal count rather than an entry count, because clubcard filters don't record how many certificates they hold.
Confirmed current is the last successful check against Remote Settings, changed or not, stored in the cache so a restart reports it; a source that rarely changes still reads fresh, a failing poller shows its age.
The time is process-wide, the counts per agent.

### Encrypted DNS is checked too

hickory 0.26's resolver builder takes a whole `rustls::ClientConfig` (`with_tls_config`), so the agent's resolver gets the same verifier as its HTTP connections, opt-outs included.
`web-faith-dns` takes the config as a rustls type and stays independent of `web-faith-revocation`.
Opportunistic probes (RFC 9539) verify nothing, so they're unaffected.
To check when building: what hickory sets on its default config (ALPN per transport, SNI) that ours must keep.

## Spec changes

Drafted: the new certificate revocation spec (REVOC), with folds into TLS, ENV, RUST, AGENT, NETCHG, OBS and DNS.

## Build notes

Points that came up while grounding the checklist in the code.

- hickory fills in ALPN per transport (`h2` for DoH, `doq` for DoQ) only when the config's `alpn_protocols` is empty, and turns SNI off on its own clone for DoT. So the DNS config is built separately from the HTTP one, with empty ALPN, rather than reusing the HTTP config that carries `h2`/`http/1.1`.
- The DNS config keeps today's DNS trust: platform roots only, and not loosened by `NODE_TLS_REJECT_UNAUTHORIZED=0`; it gains only the revocation wrapper. hickory's default (`client_config()` with `rustls-platform-verifier`) already ignores the agent's extra roots, so handing it the HTTP verifier would have widened what DNS trusts as a side effect.
- `FaithError` from a reqwest error carries the whole source chain in its message, so a refusal surfaces as `CertificateError::Other` holding a `Revoked` error of the crate's own whose `Display` names the source. rustls's unit `CertificateError::Revoked` can't carry which source refused.
- `ClientRecipe::build` is re-run on a network change, so the rustls config, the verifier and the session store are rebuilt there, while the process-wide revocation handle and the agent's refusal counters are held by the agent and passed in, like the cookie jar and resolver.
- The crate exposes a non-global `Revocation` handle (base URL, pinned root, cache dir, sources); the process-wide `OnceLock` lives in `web-faith`. That keeps the crate's own tests free of global state.
- The poller's client is built with Faith's TLS config but without the revocation wrapper: its data is protected by the content signature, and checking the poller's own connection against the data it is fetching adds nothing.
- `clubcard-crlite`'s `builder` feature (dev-dependency only) builds test filters, so CRLite tests need no captured Mozilla filter.
- Agent-level refusals are tested through an `unstable-internals` Rust hook that points the process-wide handle at a fake source; the Node tests cover option plumbing and the stats shape only, so nothing new is exposed to JS.
- CRLite filters reach the store through a provider seam: the query side takes a set of held filters and their coverage, whoever supplied them. Faith's own Remote Settings fetch is the one provider here; a system upki cache is a second, on its own card.
- x509 fields webpki doesn't expose (issuer and subject DN, serial, SPKI, SKI, AKI, the SCT list) come from a DER parser crate; `x509-cert` (RustCrypto) is the pick, as it parses without copying and has no C dependency.

## Checklist

### 1. Faith owns the rustls config (no revocation yet)

A refactor with no behaviour change, landed on its own so the existing TLS tests vouch for it before revocation is layered on.

- [x] `cargo add` `rustls` as a non-optional `web-faith` dependency, its `aws-lc-rs` / `ring` features driven by `tls-aws-lc-rs` / `tls-ring`; `rustls-platform-verifier` at the version reqwest resolves in `Cargo.lock`; `rustls-pki-types` for PEM parsing
- [x] `crates/web-faith/src/client/tls.rs`: a `TlsRecipe` that builds the `rustls::ClientConfig` with the chosen provider, the platform verifier with extra roots (agent's plus `NODE_EXTRA_CA_CERTS`) or a no-verify verifier for `NODE_TLS_REJECT_UNAUTHORIZED=0`, client auth from `tls.identity`, `KeyLogFile`, ALPN `h2` + `http/1.1`, and `enable_early_data` from `tls.early_data`
- [x] `agent/build.rs` and `NodeEnvRecipe`: parse identity and extra roots into rustls types (`CertificateDer`, `PrivateKeyDer`) instead of `reqwest::Identity` / `Certificate`, keeping `PemParse` for malformed input and the lenient `NODE_EXTRA_CA_CERTS` read
- [x] `ClientRecipe::build`: replace `identity`, `tls_certs_merge`, `danger_accept_invalid_certs`, `tls_early_data` and `tls_sslkeylogfile` with `tls_backend_preconfigured(config)`; keep `tls_info` and `https_only`
- [x] Decide whether `install_crypto_provider` is still needed once Faith passes the provider explicitly: dropped, since Faith picks the provider itself (a process default the embedder installed still wins), and step 2 hands hickory Faith's config too
- [x] Run the full Rust and Node suites; extra roots, `NODE_TLS_REJECT_UNAUTHORIZED`, mTLS, HTTP/3 and early data, ALPN negotiation of h2 must all still pass. Build with `--no-default-features --features tls-ring` too

### 2. `web-faith-dns` takes a rustls config

- [x] `FaithResolver::new_with_tls(config, tls)` takes a `rustls::ClientConfig` (rustls as a workspace dependency of the crate), applied with `with_tls_config` in each builder in `discovery.rs`, bootstrap included. A constructor rather than a `ResolverConfig` field, since that struct is exhaustive and a new field would be a semver break
- [x] Checked hickory-resolver 0.26.1's opportunistic probes: it does not swap the verifier for them; a probe connects with the resolver's TLS config as given. So handing it Faith's config changes nothing about whether a probe authenticates. In practice discovery's system servers carry no encrypted connection configs for hickory to probe
- [x] `web-faith` builds the DNS config per the build note: platform verifier, empty ALPN, same provider; wrapped with revocation in step 9

### 3. Scaffold `web-faith-revocation`

- [ ] `crates/web-faith-revocation/` with `Cargo.toml` (features `onecrl`, `crlite` = `dep:clubcard-crlite`, both default), workspace dependency entry, README, docs.rs metadata matching the other component crates
- [ ] Dependencies via `cargo add`: `rustls`, `rustls-webpki`, `reqwest`, `tokio`, `serde`, `serde_json`, `base64`, `sha2`, `x509-cert`, `rustls-native-certs`; dev: `rcgen`, `clubcard-crlite` with `builder`
- [ ] Crate error type in the style of `web-faith-alt-svc` (no `thiserror` in the workspace)
- [ ] Modules: `cert`, `links`, `remote_settings`, `signature`, `onecrl`, `crlite`, `store`, `cache`, `session`, `verifier`, `poller`

### 4. Certificate facts (`cert`)

- [ ] From a `CertificateDer`: raw issuer and subject DN, serial, SPKI DER and its SHA-256, SKI, AKI key identifier
- [ ] SCT list extension (`1.3.6.1.4.1.11129.2.4.2`): unwrap the OCTET STRING, parse the TLS-encoded list into `(LogId, Timestamp)` pairs; tolerate a malformed list as "no SCTs"
- [ ] Unit tests on rcgen certs and on one captured public leaf with SCTs committed as a fixture

### 5. Path linking (`links`)

- [ ] `RootIndex`: platform roots from `rustls-native-certs`, keyed by subject and by SKI; built off the handshake path, absent until first built (root step fails open)
- [ ] Per-agent extra roots (agent's and `NODE_EXTRA_CA_CERTS`) indexed alongside, for the HTTP verifier only
- [ ] Path enumeration from the leaf over presented certs plus index lookups: subject = issuer, SKI = AKI where both present; cycle guard and depth cap
- [ ] Tests: stale cross-sign presented alongside a valid path, root not presented, AKI mismatch skips a candidate, cycle, unknown issuer

### 6. Remote Settings and content signatures (`remote_settings`, `signature`)

- [ ] Change check against `buckets/monitor/collections/changes/changeset`, then `buckets/main/collections/{cid}/changeset?_expected={ts}` for a collection that moved
- [ ] Attachment base URL from the server root's `capabilities.attachments.base_url`
- [ ] Canonical JSON of the records as Remote Settings signs it (records sorted by id, keys sorted, no whitespace, `last_modified` alongside)
- [ ] `x5u` chain fetch and verification with webpki against the pinned root: code-signing EKU, SAN `onecrl.content-signature.mozilla.org`, validity now
- [ ] P-384 signature over `Content-Signature:\0` + canonical JSON using the provider's `ECDSA_P384_SHA384`, raw `r‖s` converted to DER
- [ ] Rollback refusal against the held timestamp
- [ ] Mozilla's root and base URL as constants in the crate, passed in by `web-faith`
- [ ] Tests: a captured real OneCRL changeset and its chain verify against the real root (fixture, no network); a tampered record, wrong SAN, wrong root, or older timestamp each fail

### 7. OneCRL (`onecrl`)

- [ ] Parse records (`issuerName` + `serialNumber`, or `subject` + `pubKeyHash`, base64) into lookup sets
- [ ] Refuse only when every linked path contains a listed certificate
- [ ] Tests over the step 5 fixtures, both entry kinds

### 8. CRLite (`crlite`)

- [ ] Parse `cert-revocations` records for the default channel: attachment hash, size and location, full vs incremental, parent
- [ ] Download attachments, check SHA-256 against the signed record, `CRLiteClubcard::from_bytes`
- [ ] Split loading from querying: a provider hands the store its filter set, and the query works from that set alone, so a second provider plugs in without touching the verifier
- [ ] Hold the full filter and every verified delta; a new full filter replaces the set only once downloaded and verified
- [ ] Query each candidate issuer key with the leaf's serial and SCTs across all held filters; refuse only when revoked under every candidate; `NotEnrolled` / `NotCovered` pass
- [ ] Tests with builder-made filters: revoked in the full filter, revoked only in a delta, missing delta, no SCTs, unknown issuer, two candidate issuers

### 9. Store, cache, session cutting, verifier

- [ ] `store`: the `Revocation` handle holding each source's current data, an epoch counter bumped on change, and the last-confirmed time per source
- [ ] `cache`: layout under the given dir (OneCRL changeset with signature and confirmed time; CRLite records plus `filters/{sha256}`), temp file and rename, re-verified on load, memory-only when unwritable
- [ ] Pruning: unnamed filters deleted after a grace period, first-unnamed time kept in the cache metadata, failed deletes retried next update
- [ ] `session`: a `ClientSessionStore` wrapper over rustls's memory cache that tags entries with the epoch at insert and drops older ones on read
- [ ] `verifier`: wraps an `Arc<dyn ServerCertVerifier>`, runs the inner first, then the enabled sources; refusal error per the build note; bumps a refusal counter handed in by the caller; signature checks, schemes and root hints pass straight through
- [ ] Tests: a full rustls handshake against an rcgen chain over loopback with a listed intermediate refused, a revoked leaf refused, opt-outs honoured, and a resumed session not resumed after an epoch bump

### 10. Poller (`poller`)

- [ ] Dedicated thread with a current-thread Tokio runtime, detached, spawned once; a channel to add sources
- [ ] Daily change check, skipped at start when the cache was confirmed within the interval; backoff on failure, last good data kept
- [ ] Root index rebuilt on the poller thread after each OneCRL load or update, and once at start
- [ ] Test against a fake Remote Settings server (hyper on 127.0.0.1, rcgen P-384 test root with the required SAN): first fetch, unchanged check updates confirmed time only, change bumps the epoch, bad signature keeps old data
- [ ] The fake server and test chain live in a `test-support` module behind a non-default feature, so `web-faith`'s tests reuse them

### 11. `web-faith` integration

- [ ] Features `onecrl` and `crlite` passing through to `web-faith-revocation`, both default; `lib.rs` feature table and README
- [ ] `TlsOptions.revocation` with `onecrl` / `crlite` booleans, each behind its feature
- [ ] Process-wide `OnceLock` holding the `Revocation` handle: created at the first agent using a source, with the `directories` user cache dir joined with `web-faith/revocation`, the cache loaded synchronously, then the poller spawned; a later agent enabling another source loads that one and hands it to the poller
- [ ] Poller's `reqwest::Client` built from the default recipe and `NodeEnvRecipe` per the build note
- [ ] `ClientRecipe` carries the handle, the agent's enabled sources and its refusal counters; HTTP verifier and DNS verifier both wrapped; no wrapper under `NODE_TLS_REJECT_UNAUTHORIZED=0` on HTTP
- [ ] `AgentStats` gains per-source refused count and last-confirmed time, each behind its feature, absent for a source turned off or without data
- [ ] `unstable-internals` test hook that points the process-wide handle at a given base URL, pinned root and cache dir; it must be called before the first agent that uses a source, and returns an error after that
- [ ] Rust integration tests in their own test binary (the handle is process-wide) using the hook and the step 10 fake server: an agent refuses a revoked leaf and a listed intermediate with a `Network` error naming the source, opt-outs let them through, stats count the refusals and report last-confirmed, a restart from the cache enforces before any fetch
- [ ] Rust tests: options with features off fail to compile (a `compile_fail` doctest)

### 12. Node surface

- [ ] `web-faith-napi` features `onecrl` and `crlite`, both in its default set
- [ ] `AgentTlsOptions.revocation { onecrl?, crlite? }` in `agent/options.rs`, mapped in `agent/convert.rs`
- [ ] napi `AgentStats` gains the revocation fields, following the existing stats field conventions
- [ ] Rebuild to regenerate `index.d.ts`; update `wrapper.d.ts` if it restates the option types
- [ ] Tape tests: options accepted, stats shape with sources on and off

### 13. Release plumbing

- [ ] `release-plz.toml` comment says seven published crates; check whether the new crate needs an entry
- [ ] Top-level README mentions revocation checking and the MPL-2.0 `clubcard-crlite` dependency behind `crlite`
