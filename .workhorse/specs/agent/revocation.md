---
id: REVOC
---

# Certificate revocation

On top of the platform's own verification, Faith checks server certificates against Mozilla's published revocation data, which it fetches and keeps current while the process runs.
A certificate Mozilla revokes is refused once the next update lands, without a new Faith release.
Two sources are checked: OneCRL for intermediate and root certificates, and CRLite for leaf certificates.

## Where the check sits

Revocation is checked after the platform verifier has accepted a certificate chain, and only ever adds rejections: a chain the platform verifier refuses stays refused, and one it accepts is refused only when a source says it is revoked.
This holds on every platform, on top of whatever revocation checking the platform verifier does itself.
Every TLS connection an agent makes for HTTP is checked, over TCP and over QUIC alike, including the connections its background requests open.
So is every connection its resolver makes to an encrypted DNS server it authenticates; an opportunistic probe authenticates nothing, so there is no chain to check (see [DNS](dns.md)).

A resumed TLS session skips certificate verification, so a session saved before the revocation data last changed is not resumed: the connection makes a full handshake instead, and is checked against the current data.
A connection already open when an update lands carries on until it closes; the update applies to the connections opened after it.

A connection refused for revocation fails its request with the code `Network`, as any other certificate rejection does (see [ERR](../errors/errors.md)), and its message says the certificate is revoked and names the source that refused it.
A DNS server refused for revocation fails the lookup as any other authentication failure does.

## Sources

Both sources are fetched from Mozilla Remote Settings: OneCRL from the `security-state/onecrl` collection, CRLite from the default channel of `security-state/cert-revocations`.

OneCRL lists intermediate and root certificates Mozilla has revoked, each entry naming either an issuer and serial number or a subject and public key hash.
CRLite covers leaf certificates issued by the publicly trusted authorities enrolled in it, as a full filter plus incremental filters published since, each listing the revocations of its own window.

## Linking certificates

The platform verifier does not say which path it built, and servers often send certificates beyond the path that validates, such as stale cross-signs, while leaving out the root.
So the check works out every path that could have been built, from the leaf upwards, linking each certificate to every candidate that could have issued it: one whose subject is its issuer, and whose key identifier matches its authority key identifier where both carry one.
Candidates are the certificates the server presented and the roots of the agent's trust store, the platform's included, as Faith can list them.
A root the platform would fetch for itself rather than holding locally, and an intermediate it found for itself rather than receiving from the server, cannot be listed, so a path through one ends before it and the missing certificate is not checked.

## Matching OneCRL

A connection is refused only when every path linked this way contains a certificate OneCRL lists.
An issuer-and-serial entry matches a certificate with that issuer and serial number, and a subject-and-key-hash entry one with that subject and public key.

## Matching CRLite

CRLite is checked for the leaf certificate, by its serial number and its issuer's public key, against the Certificate Transparency timestamps the leaf carries.
The issuer is found by the same linking as OneCRL, and where more than one candidate could have issued the leaf, it is refused only when it is revoked under every candidate's key.
A leaf is refused when any filter held lists it as revoked.

A leaf the data does not cover passes:

- one whose issuer cannot be found among the candidates
- one whose issuer is not enrolled in CRLite
- one carrying no Certificate Transparency timestamps, as certificates from private authorities generally do
- one logged after the data was built

## Integrity

Data is used only once its Remote Settings content signature verifies, as Firefox requires.
The signing certificate chain must lead to Mozilla's content-signing root, which Faith pins, and the signing certificate must name `onecrl.content-signature.mozilla.org`, the signer for both collections.
CRLite filters are downloaded separately from the records that describe them, and a filter is used only when its hash matches the one in its signed record.
Data read back from the cache is held to the same checks as data fresh from the network.
Data signed earlier than the data already held is refused, so a replayed older collection cannot strip entries added since; a process with no data yet has nothing to compare against and accepts any collection that verifies.

## Keeping current

Every agent in a process shares one copy of each source's data and one poller, so many agents cost one fetch.
The poller runs on a thread of its own, independent of any async runtime the caller has, and lasts as long as the process without keeping it alive.
It starts with the construction of the first agent that uses a source, right after that agent loads the cache, and takes on a source the first time an agent using it is constructed.

The poller checks Remote Settings for changes about once a day, as Firefox does, and downloads only what has changed.
A failed fetch, a signature that does not verify, or a filter whose hash does not match leaves the data already held in force, and the poller tries again later.
Held data stays in force however long it goes without being confirmed current; its age shows in `stats()` (see [Reporting](#reporting)).

The poller connects to Mozilla's Remote Settings service at `firefox.settings.services.mozilla.com` and to the host that service names for CRLite filter downloads.
It belongs to the process rather than to any agent, so it takes its network settings from the environment alone (see [ENV](../environment/variables.md)) and none of any agent's options, and its requests move no agent's counters.

A missing incremental filter loses only the revocations of its own window, so the filters that are held are used.
A new full filter replaces the old one and the incremental filters published against it once it has downloaded and verified, and until then the old set stays in force.

## The cache

Each verified update is written to a cache on disk.
The first agent in a process that uses a source loads that source's cached data while it is constructed, so a restart enforces what was last known from its first connection, without waiting for the network.
Later agents share what the first one loaded.
The cache lives in the platform's per-user cache directory (see [ENV](../environment/variables.md) for how it is located).
Processes on one host share it: a CRLite filter is stored under its hash, so one process's download serves the others, and each file is written whole before it becomes visible, so no process reads a partial one.
After each verified update, a stored filter that the current signed records no longer name is deleted once it has gone unnamed for a grace period long enough for other processes to have moved past it; a deletion that fails is left for the next update.
Where the cache directory is missing or cannot be written, the data is held in memory only and each process fetches its own.
The cache records when its data was last confirmed current, so a process starting from it reports that time.
Until a source has any data, from the cache or the network, it refuses nothing, so on a host with no cache yet the connections made before the first fetch lands go unchecked.

## Reporting

Each agent's `stats()` reports, for each source the agent uses, how many of its connections that source has refused and when the data was last confirmed current (see [OBS](observability.md)).
Confirmed current means a check against Remote Settings succeeded, whether or not it found a change, so a source that rarely changes still reads as fresh while a poller that keeps failing shows its age.

## Opting out

`tls.revocation.onecrl` and `tls.revocation.crlite` each turn one source off for an agent; both default to on.
An agent with a source off does not check it and does not have the poller fetch it, so a process in which no agent uses CRLite never downloads it.
An agent with certificate validation turned off checks neither source (see [ENV](../environment/variables.md)).
On the Rust surface each source is also a compile-time feature, and a build without one has neither its check nor its option (see [RUST](../rust/overview.md)).
