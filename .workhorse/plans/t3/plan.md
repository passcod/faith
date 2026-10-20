# Certificate revocation checking

Check server certificates against a browser-style revocation source, on by default with an opt-out.
Node gets the data at runtime; Rust can also embed a snapshot at compile time.

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
| Compile-time snapshot | viable | stale within a day |
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

### Separate opt-outs

Each source has its own opt-out.
CRLite is the heavy one (a ~6 MB first download, ~500 KB of deltas a day, ~6 to 7 MB resident), so callers can keep OneCRL while dropping it.
Opting out of a source stops both its checks and its polling.

### Background polling, one process-wide set

Faith polls Remote Settings itself, using the cheap change check, at roughly Firefox's daily cadence.
The set and the poller are process-wide and shared by every agent, so many agents in one process cost one fetch.
Polling only runs for sources some agent in the process has left on.

### On-disk cache for cold start

Each verified set is written to a cache directory and loaded at startup, so a restart enforces the last known set straight away.
Before any set is known (a fresh machine, or an unwritable cache), nothing is enforced until the first fetch lands.

No compile-time snapshot: webpki's own distrust releases already cover the window it would, so embedding one adds nothing.

### Content signatures are verified

Fetched data is checked against its Remote Settings content signature before use, as Firefox does: canonical JSON of the records, `x5u` chain to a pinned Mozilla content-signing root, SAN `onecrl.content-signature.mozilla.org` (which signs both collections), P-384 ECDSA.
The disk cache keeps the signature alongside the records and is re-verified on load.
This stops someone who can intercept TLS to Mozilla from stripping entries, which is the same attacker a revoked intermediate helps.

### OneCRL matching walks issuer links over the presented certificates

The platform verifier says a trusted path exists but not which one, and servers often send stale cross-signs alongside it.
So from the leaf, the check links each certificate to the presented certificates that could have issued it (subject matches issuer, and SKI matches AKI where both exist), and repeats upwards.
A connection is rejected only when every chain built this way hits a OneCRL entry.
Issuer-and-serial entries match on those fields, subject-and-key-hash entries on theirs.

Linkage is by name and key ID, not signatures: the platform verifier has already proved a valid path, this only works out which presented certificates could be on it.
It behaves the same on every platform.
An issuer the OS fetched itself (AIA) isn't checked, so that gap fails open, like CRLite's missing coverage.
