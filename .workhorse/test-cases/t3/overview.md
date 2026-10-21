# Certificate revocation checking

Scenarios for OneCRL and CRLite checking. Refusals are tested in Rust against a fake Remote Settings server; the Node tests cover option plumbing and the stats shape.

## Refusals

- [x] A chain whose intermediate OneCRL lists is refused with a `Network` error naming OneCRL (verifies spec: REVOC)
- [x] A leaf CRLite lists as revoked is refused with a `Network` error naming CRLite (verifies spec: REVOC)
- [x] A clean chain from the same test setup is accepted (verifies spec: REVOC)
- [x] OneCRL entries match by issuer and serial, and by subject and key hash; a disabled record is still enforced (verifies spec: REVOC)
- [x] A stale cross-sign the server sends alongside a valid path does not cause a refusal; listing every path's certificate does (verifies spec: REVOC)
- [x] A root the server leaves out is found in the trust store and checked (verifies spec: REVOC)
- [x] A candidate whose key identifier does not match the leaf's authority key identifier is not linked (verifies spec: REVOC)
- [x] Mutually cross-signed certificates terminate the walk (verifies spec: REVOC)
- [x] CRLite passes a leaf with no SCTs, with an issuer it cannot find, and with an issuer not enrolled (verifies spec: REVOC)
- [x] With two candidate issuer keys, CRLite refuses only when the leaf is revoked under both (verifies spec: REVOC)
- [ ] An encrypted DNS server whose certificate is revoked fails the lookup (verifies spec: DNS, REVOC)
- [ ] An HTTP/3 connection to a revoked chain is refused (verifies spec: REVOC)

## Integrity

- [x] A captured real `cert-revocations` changeset verifies against Mozilla's pinned root (verifies spec: REVOC)
- [x] A dropped record, a changed timestamp, a different root, an expired signing chain, or a signing certificate naming another signer fails verification (verifies spec: REVOC)
- [x] A collection older than the one held is refused (verifies spec: REVOC)
- [x] A CRLite filter whose bytes do not match its signed record is not used (verifies spec: REVOC)
- [x] Live Mozilla data fetches, verifies and loads (checked by hand against production)

## Keeping current

- [x] An unchanged collection is confirmed without downloading it again (verifies spec: REVOC)
- [x] A new incremental filter is picked up without downloading the full filter again (verifies spec: REVOC)
- [x] An incremental filter that failed to download is retried, and the rest of the data is used meanwhile (verifies spec: REVOC)
- [x] A collection with a bad signature leaves the held data in force (verifies spec: REVOC)
- [x] A session saved before an update is not resumed, so the next connection is checked (verifies spec: REVOC)
- [ ] The poller reaches Mozilla through a proxy set in the environment (verifies spec: ENV)

## Cache

- [x] A restart enforces cached data before any fetch (verifies spec: REVOC)
- [x] A restart on a cache confirmed within the interval makes no request (verifies spec: REVOC)
- [x] A restart reports the cache's last-confirmed time (verifies spec: REVOC)
- [x] A tampered cache file is not used (verifies spec: REVOC)
- [x] Filters no record names are deleted only after the grace period, which spans restarts (verifies spec: REVOC)
- [x] An unwritable cache directory leaves the data in memory without error (verifies spec: REVOC)
- [ ] Two processes sharing one cache directory: the second uses the first's filters without downloading them (verifies spec: REVOC)

## Opting out and reporting

- [x] Turning a source off lets through what only it refuses (verifies spec: REVOC)
- [x] Node: each source can be turned off, and an empty `revocation` group keeps the defaults (verifies spec: REVOC)
- [x] Node: `NODE_TLS_REJECT_UNAUTHORIZED=0` turns both sources off (verifies spec: ENV, REVOC)
- [x] `stats()` reports refusals per source and when the data was last confirmed, and `null` for a source turned off (verifies spec: OBS)
- [ ] `stats()` counts a refused DNS server connection (verifies spec: OBS)
- [x] A Rust build without a source has no option for it (compile-fail doctest, run in such a build)
