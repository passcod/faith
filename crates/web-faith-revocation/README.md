# web-faith-revocation

Certificate revocation checking for [rustls], from Mozilla's [OneCRL] and [CRLite], on top of any
other certificate verifier.

`RevocationVerifier` wraps a `ServerCertVerifier`, such as the platform verifier, and refuses a
chain that verifier accepted when a revocation source lists a certificate on it:

- **OneCRL** lists the intermediate and root certificates Mozilla has revoked.
- **CRLite** covers leaf certificates from the publicly trusted authorities enrolled in it.

Both are fetched from Mozilla Remote Settings, checked against their content signature, kept
current by a background poller, and cached on disk, so a restart enforces the last known data from
its first connection.

Where [upki] keeps CRLite data current for the whole system, the `upki` feature reads that cache
instead of fetching CRLite. upki's manifest is not signed, so that data is trusted as the system's
own, like its trust store, and the signed fetch from Remote Settings is the fallback when the cache
is missing, damaged or stale.

The verifier works out every path the wrapped verifier could have built, from the presented
certificates and the trust store's roots, and refuses only when every one of them holds a revoked
certificate, so a stale cross-sign sent alongside a valid path does not cause a refusal.

This crate is part of [Faith], which uses it for every agent's TLS connections.

## Features

| Feature | Default | What it adds |
| --- | :-: | --- |
| `onecrl` | ✓ | The OneCRL source. |
| `crlite` | ✓ | The CRLite source, and its MPL-2.0 filter decoder `clubcard-crlite`. |
| `upki` | ✓ | Reading CRLite data from the system's upki cache. |
| `test-support` |  | A fake Remote Settings server and signing chain, for tests. |

[rustls]: https://docs.rs/rustls
[OneCRL]: https://wiki.mozilla.org/CA/Revocation_Checking_in_Firefox#OneCRL
[CRLite]: https://github.com/mozilla/crlite
[upki]: https://github.com/rustls/upki
[Faith]: https://github.com/passcod/faith
