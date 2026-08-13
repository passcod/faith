---
id: SRI
---

# Subresource integrity

The `integrity` request option carries a subresource integrity value, and Faith verifies the response body against it.
On a server there is no document to protect, but the same mechanism guards artefact downloads and API responses against tampering and corruption.

## Accepted values

The option takes the standard SRI format `<hash-algo>-<hash-source>`, with `sha256`, `sha384`, and `sha512` as the known algorithms and the base64-encoded digest as the source.
Multiple space-separated values are accepted.
Verification uses the strongest algorithm listed, ranking `sha512` above `sha384` above `sha256`, and checks the body against the digests given for that algorithm alone; it passes if any of those matches.
A value that lists both a weak and a strong digest is therefore held to the strong one, and a body matching only the weak digest fails verification.
Unknown algorithms are silently ignored when picking the strongest; if every listed algorithm is unknown, the request throws an invalid-integrity error at parse time.

## When verification happens

Verification runs when the body is consumed through `bytes()`, `json()`, `text()`, `arrayBuffer()`, or `blob()`: the paths where the whole body is in hand.
The digest is taken over the bytes the caller receives, which are the encoded bytes on a response Faith does not decode (see [ENC](content-encoding.md)).
A mismatch throws an integrity-mismatch error from that call.
Reading through the `body` stream does not verify: the consumer sees bytes before the digest can be known, so stream consumers take on their own verification.
Because the body is not available at `fetch()` resolution time, a mismatch surfaces at the body-reading call, not at `fetch()`.
Browsers throw earlier because they buffer; the failure mode is the same, only the timing differs.
