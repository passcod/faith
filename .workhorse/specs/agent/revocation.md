---
id: REVOC
---

# Certificate revocation

On top of the platform's own verification, Faith checks server certificates against Mozilla's published revocation data, which it fetches and keeps current while the process runs, or reads from the system's upki cache where one is kept current for it.
A certificate Mozilla revokes is refused once the next update lands, without a new Faith release.
Two sources are checked: OneCRL for intermediate and root certificates, and CRLite for leaf certificates.

## Where the check sits

Revocation is checked after the platform verifier has accepted a certificate chain, and only ever adds rejections: a chain the platform verifier refuses stays refused, and one it accepts is refused only when a source says it is revoked.
This holds on every platform, on top of whatever revocation checking the platform verifier does itself.
Every TLS connection an agent makes for HTTP is checked, over TCP and over QUIC alike, including the connections its background requests open.
So is every connection its resolver makes to an encrypted DNS server it authenticates; an opportunistic probe authenticates nothing, so there is no chain to check (see [DNS](dns.md)).

A resumed TLS session skips certificate verification, so a session saved before the revocation data last changed is not resumed: the connection makes a full handshake instead, and is checked against the current data.
A session counts as saved at the time its connection's certificate was verified, not when its ticket arrived, since a ticket can reach a long-lived connection after the data has changed.
A connection already open when an update lands carries on until it closes; the update applies to the connections opened after it.

A connection refused for revocation fails its request with the code `Network`, as any other certificate rejection does (see [ERR](../errors/errors.md)), and its message says the certificate is revoked and names the source that refused it.
A DNS server refused for revocation fails the lookup as any other authentication failure does.

## Sources

Both sources are fetched from Mozilla Remote Settings: OneCRL from the `security-state/onecrl` collection, CRLite from the default channel of `security-state/cert-revocations`.
CRLite has a second provider, the system's upki cache, which carries the same Mozilla data and is preferred where it is usable (see [The system's CRLite data](#the-systems-crlite-data)).
OneCRL always comes from Remote Settings.

OneCRL lists intermediate and root certificates Mozilla has revoked, each entry naming either an issuer and serial number or a subject and public key hash.
CRLite covers leaf certificates issued by the publicly trusted authorities enrolled in it, as a full filter plus incremental filters published since, each listing the revocations of its own window.

## Linking certificates

The platform verifier does not say which path it built, and servers often send certificates beyond the path that validates, such as stale cross-signs, while leaving out the root.
So the check works out every path that could have been built, from the leaf upwards, linking each certificate to every candidate that could have issued it: one whose subject is its issuer, and whose key identifier matches its authority key identifier where both carry one.
Candidates are the certificates the server presented and the roots of the agent's trust store, the platform's included, as Faith can list them.
A path ends at a self-issued certificate, so a root is never linked on to a cross-sign of itself.
The platform's roots are listed while the first agent that uses a source is constructed.
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

Data Faith fetches itself is used only once its Remote Settings content signature verifies, as Firefox requires.
Data read from a upki cache is not signed and is held to the checks in [The system's CRLite data](#the-systems-crlite-data) instead.
The signing certificate chain must lead to Mozilla's content-signing root, which Faith pins, and the signing certificate must name `onecrl.content-signature.mozilla.org`, the signer for both collections.
CRLite filters are downloaded separately from the records that describe them, and a filter is used only when its hash matches the one in its signed record.
Data read back from the cache is held to the same checks as data fresh from the network.
Data signed earlier than the data already held is refused, so a replayed older collection cannot strip entries added since; a process with no data yet has nothing to compare against and accepts any collection that verifies.

## Keeping current

Every agent in a process shares one copy of each provider's data and one poller, so many agents cost one fetch.
The poller starts with the first agent that uses a source and lasts as long as the process, without keeping the process alive.
It fetches CRLite only while some agent's CRLite data is not supplied by a usable, fresh upki cache.

The poller checks Remote Settings for changes about once a day, as Firefox does, and downloads only what has changed.
A failed fetch, a signature that does not verify, or a filter whose hash does not match leaves the data already held in force, and the poller tries again later.
A fetch that stalls is abandoned after a timeout and counts as failed.
A new agent using a source already followed does not bring a retry forward.
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
For CRLite it also names the provider of the data in force, `remote-settings` for Faith's own fetch or `upki` for the system's cache, and reports nothing until there is data.
Confirmed current means a check against Remote Settings succeeded, whether or not it found a change, so a source that rarely changes still reads as fresh while a poller that keeps failing shows its age.
For upki data it is the time of upki's last successful fetch, when it last confirmed its own data current in the same sense.

## The system's CRLite data

Where upki is installed, its cache is the preferred provider of CRLite data: one system-wide copy that the system keeps current, from the mirror its administrator chose, which a container can use by mounting the host's cache without downloading anything.
A usable, fresh upki cache is the only CRLite data in force for an agent that uses upki, and the poller does not fetch CRLite on its behalf.
Faith reads the cache and never writes to it, runs upki, or contacts its mirror.

### Locating the cache

Faith locates the cache as upki does.
The configuration file is the user's own `upki/config.toml` in the per-user configuration directory when that exists, and otherwise the system one: `/etc/upki/config.toml` on Unix, `/Library/Application Support/upki/config.toml` on macOS, and `upki\config.toml` under `%ProgramData%\rustls` on Windows.
Its `cache-dir` names the directory that holds the cache, and the CRLite data is in that directory's `revocation` subdirectory.
Without a configuration file the cache directory is upki's per-user cache directory where that exists, and otherwise its system one: `/var/cache/upki`, `/Library/Caches/dev.rustls.upki`, or `upki\cache` under `%ProgramData%\rustls`.
A configuration file that exists but cannot be read or parsed, or that names no `cache-dir`, leaves the cache unlocated, and the system's file is not consulted in place of a user's that does.
The location is worked out afresh at each check of the cache, so a cache installed or reconfigured while the process runs is found.

### A usable cache

A cache is usable when its `index.bin` and `manifest.json` both read and decode, the index names at least one filter, and every filter the index names is present, is listed in the manifest, matches the manifest's hash, and decodes.
A filter name that is not a plain file name inside the cache directory makes the cache unusable.
A cache that is not usable is treated as absent, however it fell short, and Faith falls back to its own data; a cache read while an update is replacing its files is read again from the start before it counts as unusable.
The hashes catch a damaged or partly replaced cache.
They are not a defence against one that has been tampered with, since the manifest that carries them is not signed.

### A fresh cache

A usable cache is fresh while its last successful fetch is no more than three days old, measured from the modification time of its manifest, which every successful fetch rewrites whether or not the data changed.
A cache older than that is stale, and Faith fetches its own data meanwhile.
Stale data stays in force until Faith's own data is held and was confirmed current more recently, so there is no gap while the first fetch lands.
When the cache is refreshed and found fresh again, it takes over once more and the poller stops fetching CRLite.
Faith's own data stays in its own cache on disk through any of this.

### Keeping up with the cache

The first agent in a process that uses CRLite reads the upki cache while it is constructed, as it does Faith's own cache, so a restart enforces what upki last fetched from its first connection.
The poller then checks the cache about every ten minutes, reading nothing from the network: when the manifest's modification time has changed it reads the cache again, and replaces the upki data held only once the new data is usable.
A reload that finds the cache unusable leaves the data already held in force.
The same check notices a cache that has appeared, a cache that has gone stale, and one that has gone absent: data already held stays in force and ages as any other does.
A reload that replaces data moves the data's epoch, so sessions saved before it are not resumed, as with any other update (see [Where the check sits](#where-the-check-sits)).

### Trust

Upki data is trusted as the system's own.
Its integrity rests on the authenticated connection upki fetches it over and on who may write the cache directory, as the platform's trust store's does, and Faith does not inspect the directory's permissions.
Data signed earlier than the data already held cannot be recognised, so the guard against replayed older data in [Integrity](#integrity) does not apply to it.

### Choosing between providers

An agent that uses upki is given the upki data while it is usable and fresh, and Faith's own data otherwise, as above.
An agent that does not use upki is given Faith's own data alone, and the poller keeps it current.
The two providers' data is never combined: a leaf is checked against one provider's filters or the other's.

## Opting out

`tls.revocation.onecrl` and `tls.revocation.crlite` each turn one source off for an agent; both default to on.
`tls.revocation.upki` turns off the use of the system's upki cache for an agent; it defaults to on, and has no effect on an agent with CRLite off.
An agent with a source off does not check it and does not have the poller fetch it, so a process in which no agent uses CRLite never downloads it.
An agent with upki off has CRLite data from Faith's own fetch alone, and has the poller fetch it even where a upki cache is in force for other agents in the process.
An agent with certificate validation turned off checks neither source (see [ENV](../environment/variables.md)).
On the Rust surface each source is also a compile-time feature, and a build without one has neither its check nor its option (see [RUST](../rust/overview.md)).
The reading of the upki cache is a compile-time feature of its own, and a build without it has neither the reader nor `tls.revocation.upki`.
