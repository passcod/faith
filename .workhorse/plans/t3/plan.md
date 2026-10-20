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

It is spawned on the Tokio runtime current at the first handshake that needs it, and respawned on the next one if that runtime goes away, since Rust callers can run many short-lived runtimes.
This is the pattern `web-faith-dns` and `web-faith-alt-svc` already use for their background work; `web-faith` owns no runtime.
On Node it lands on napi's `tokio_rt` runtime, which lives for the process and doesn't hold the event loop open.
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
