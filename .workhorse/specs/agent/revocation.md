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
A connection refused for revocation fails its request with the code `Network`, as any other certificate rejection does (see [ERR](../errors/errors.md)), and a DNS server refused for revocation fails the lookup as any other authentication failure does.

## Sources

Both sources are fetched from Mozilla Remote Settings: OneCRL from the `security-state/onecrl` collection, CRLite from the default channel of `security-state/cert-revocations`.

OneCRL lists intermediate and root certificates Mozilla has revoked, each entry naming either an issuer and serial number or a subject and public key hash.
CRLite covers leaf certificates issued by publicly trusted authorities, as a full filter plus a chain of incremental updates applied on top of it.

## Matching OneCRL

Servers often send certificates beyond the path that validates, such as stale cross-signs, and the platform verifier does not say which path it built.
So the presented certificates are linked from the leaf upwards, each to every presented certificate that could have issued it: one whose subject is its issuer, and whose key identifier matches its authority key identifier where both carry one.
A connection is refused only when every chain built this way contains a certificate OneCRL lists.
An issuer certificate the platform verifier found for itself rather than receiving from the server is not part of any such chain, so it is not checked.

## Matching CRLite

CRLite is checked for the leaf certificate, by its issuer and serial number against the Certificate Transparency timestamps the leaf carries.
A leaf the data does not cover passes: one carrying no Certificate Transparency timestamps, as certificates from private authorities generally do, or one logged after the data was built.

## Integrity

Data is used only once its Remote Settings content signature verifies, as Firefox requires.
The signing certificate chain must lead to Mozilla's content-signing root, which Faith pins, and the signing certificate must name `onecrl.content-signature.mozilla.org`, the signer for both collections.
CRLite filters are downloaded separately from the records that describe them, and a filter is used only when its hash matches the one in its signed record.
Data read back from the cache is held to the same checks as data fresh from the network.

## Keeping current

Every agent in a process shares one copy of each source's data and one poller per source, so many agents cost one fetch.
A source's poller starts when the first agent that uses it makes its first connection, and runs for the life of the process.
It checks Remote Settings for changes about once a day, as Firefox does, and downloads only what has changed.
A failed fetch, a signature that does not verify, or a filter whose hash does not match leaves the data already held in force, and the poller tries again later.

## The cache

Each verified update is written to a cache on disk.
The first agent in a process that uses a source loads that source's cached data while it is constructed, so a restart enforces what was last known from its first connection, without waiting for the network.
Later agents share what the first one loaded.
The cache lives in the platform's per-user cache directory (see [ENV](../environment/variables.md) for how it is located).
Processes on one host share it: a CRLite filter is stored under its hash, so one process's download serves the others, and each file is written whole before it becomes visible, so no process reads a partial one.
Where the cache directory is missing or cannot be written, the data is held in memory only and each process fetches its own.
The cache records when its data was last confirmed current, so a process starting from it reports that time.
Until a source has any data, from the cache or the network, it refuses nothing, so on a host with no cache yet the connections made before the first fetch lands go unchecked.

## Reporting

Each agent's `stats()` reports, for each source the agent uses, how many of its connections that source has refused and when the data was last confirmed current (see [OBS](observability.md)).
Confirmed current means a check against Remote Settings succeeded, whether or not it found a change, so a source that rarely changes still reads as fresh while a poller that keeps failing shows its age.

## Opting out

`tls.revocation.onecrl` and `tls.revocation.crlite` each turn one source off for an agent; both default to on.
An agent with a source off does not check it and does not start its poller, so a process in which no agent uses CRLite never downloads it.
An agent with certificate validation turned off checks neither source (see [ENV](../environment/variables.md)).
