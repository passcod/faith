---
id: ERR
---

# Errors

Faith produces fine-grained internal errors and maps them onto a small set of JavaScript error shapes for fetch compatibility, while preserving the precise kind as a stable `code` property.
Callers match on `error.code` against the exported `ERROR_CODES` map rather than string-matching messages or relying on coarse `instanceof` checks.

## The code contract

Every error Faith throws carries a `code` set to a stable name for its kind.
`ERROR_CODES` is exported and enumerates the library's error codes; it is generated from the same source as the errors themselves, so the two cannot drift.
Every code in `ERROR_CODES` is reachable: each one names a kind that some failure surfaces to the caller, so a branch written for any code in the map can fire.
Error messages are prefixed with the kind name and may embed underlying detail; the message is for humans, the code is the API.
Errors surfaced through reading the response `body` stream carry no `code`.

## Mapping

The codes below are the whole of `ERROR_CODES`, and the reference documentation shipped with the library (the TypeScript typings and the README) lists the same codes under the same JavaScript error names.
Abort-shaped failures throw an error named `AbortError`: `Aborted` (cancelled via `signal`) and `Timeout` (any timeout).
Network-shaped failures throw an error named `NetworkError`: `Network` covers transport and protocol failures, `Redirect` a redirect refused by an agent whose redirect policy is `error` (see [REDIR](../fetch/redirects.md)), and `ContentLengthOverrun` a body being written to a file exceeding the length the server advertised (see [BODY](../response/reading-the-body.md)).
A redirect that fails for a reason the agent did not ask for, such as exhausting the hop limit, is a transport failure and carries `Network`.
Malformed input that has a textual syntax throws `SyntaxError`: `AddressParse` (DNS override or local address), `InvalidIntegrity` (SRI value), `JsonParse` (`response.json()`), `PemParse` (TLS identity or extra roots).
API misuse throws `TypeError`: `InvalidHeader`, `InvalidMethod`, `InvalidUrl`, `Closed` (request on a closed agent), `ResponseAlreadyDisturbed` (reading a body that has been consumed or discarded, see [BODY](../response/reading-the-body.md)), and `ResponseBodyNull` (writing a response that cannot carry a body to a file).
The remainder throw a generic `Error`: `BodyStream` (internal stream handling), `Config` (invalid agent configuration), `IntegrityMismatch` (SRI digest mismatch), `FileExists` (a file write refusing an occupied destination), and `FileWrite` (the filesystem refusing a write).
