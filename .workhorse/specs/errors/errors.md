---
id: ERR
---

# Errors

Fáith produces fine-grained internal errors and maps them onto a small set of JavaScript error shapes for fetch compatibility, while preserving the precise kind as a stable `code` property.
Callers match on `error.code` against the exported `ERROR_CODES` map rather than string-matching messages or relying on coarse `instanceof` checks.

## The code contract

Every error Fáith throws carries a `code` set to a stable name for its kind.
`ERROR_CODES` is exported and enumerates the library's error codes; it is generated from the same source as the errors themselves, so the two cannot drift.
Error messages are prefixed with the kind name and may embed underlying detail; the message is for humans, the code is the API.
A kind raised from inside the HTTP stack keeps its code when the failure surfaces to JavaScript: where the transport wraps it in an error of its own, the originating kind is recovered from that error's source chain and reported, in preference to classifying the wrapper.
Errors surfaced through reading the response `body` stream carry no `code`.

## Mapping

Abort-shaped failures throw an error named `AbortError`: `Aborted` (cancelled via `signal`) and `Timeout` (any timeout).
Network-shaped failures throw an error named `NetworkError`: `Network` (transport and protocol failures, including exceeding the redirect limit) and `Redirect` (a redirect returned to an agent whose redirect policy is `error`, see [REDIR](../fetch/redirects.md)).
Malformed input that has a textual syntax throws `SyntaxError`: `AddressParse` (DNS override or local address), `InvalidIntegrity` (SRI value), `JsonParse` (`response.json()`), `PemParse` (TLS identity or extra roots).
API misuse throws `TypeError`: `InvalidHeader`, `InvalidMethod`, `InvalidUrl`, `Closed` (request on a closed agent), and `ResponseAlreadyDisturbed` (reading a body that has been consumed or discarded, see [BODY](../response/reading-the-body.md)).
The remainder throw a generic `Error`: `BodyStream` (internal stream handling), `Config` (invalid agent configuration), and `IntegrityMismatch` (SRI digest mismatch).
