# Follow-ups from the state-of-Fáith audit

Issues surfaced while speccing the current state of Fáith on this card. Each was verified against a built module before being listed; this card changes no code, so the fixes land as their own cards. The specs describe Fáith as it behaves today, so every entry that changes behaviour names the spec section it changes with it.

## Memoise the body stream in the wrapper · U1

The wrapper's `body` getter calls the native `body()` on every access, minting a fresh stream each time, and a handle taken after partial consumption replays the full body from the start. That is a same-response double read with no `clone()`, which the standard's once-only model forbids. Memoise the `ReadableStream` in the getter the way `headers` is memoised, giving the standard's same-object semantics and making the replay path unreachable from the public API. Callers requiring the raw binding directly would still get replaying handles; decide whether that needs closing too. Spec BODY, "The body stream", records the replaying behaviour today and changes with the fix.

## Surface the Redirect error code · V1

With `redirect: "error"`, the rejection arrives as `name: NetworkError, code: Network`. The redirect policy constructs the `Redirect` kind, but it round-trips through a `reqwest::Error` and the conversion re-maps it to `Network`, so the `Redirect` code exported in `ERROR_CODES` never reaches JS. Unwrap the inner FaithError (it is visible in the error's source chain) so the code survives the trip. Spec ERR, "Mapping", folds this case into `Network` today and gains the `Redirect` code with the fix.

## Prune or wire up the dead error codes · W1

`ResponseBodyNotAvailable` and `RuntimeThread` have no construction site anywhere in `src/` yet are exported in `ERROR_CODES`. Decide whether to remove them from the enum or wire them where they were meant to fire. While in there, the mapping table in the `error.rs` doc comment omits `AddressParse`, `IntegrityMismatch`, and `InvalidIntegrity`; bring it in line with the enum. Spec ERR, "Mapping", lists only the codes that can currently be thrown, and gains any code that gets wired up.

## Decide the HTTP/3 attempt-timeout retry semantics · X1

When `upgradeAttemptTimeout` expires, the cloned request is retried over TCP regardless of method. A timed-out attempt may nonetheless have been delivered, so a non-idempotent request (POST) can be executed twice. Decide whether to bless that as documented behaviour or to guard it, for example by only auto-retrying idempotent methods and surfacing an error for the rest, then update spec H3UP to record the rule.

## Honour a caller's Accept-Encoding · Y1

A response in a known encoding is decoded whatever the request advertised, so a caller who sets `Accept-Encoding` themselves, including `identity`, has no way to obtain the bytes as sent. Decode only when Fáith negotiated the encoding itself: when the caller supplies the header, deliver the body as received with `Content-Encoding` and `Content-Length` intact. Spec ENC, "Decoding", records the unconditional behaviour today and changes with the fix.

## Normalise only the methods the fetch standard normalises · A2

Every request method is uppercased, so a custom method reaches the server in upper case and a server routing case-sensitively on it sees the wrong request. The fetch standard normalises only `DELETE`, `GET`, `HEAD`, `OPTIONS`, `POST`, and `PUT`, passing any other method through as given. Match that, keeping the invalid-method rejection as it is. Spec REQ, "Method and headers", records the divergence today and drops it with the fix.

## Verify the strongest integrity algorithm · B2

When `integrity` carries several values, verification passes if any one of them matches, so a caller who lists both a weak and a strong digest gets the weaker guarantee. Subresource Integrity requires picking the strongest algorithm listed and verifying against that alone. Match it, keeping the parse-time rejection of a value whose algorithms are all unknown. Spec SRI, "Accepted values", records the divergence today and drops it with the fix.

## Add spec trace comments to the source · C2

The workspace spec rules require code to tie back to the requirement it answers with an inline `spec:ID#fragment` comment, and there is not one in `src/`. Work through the source adding them where a spec requirement is actually implemented: option validation in agent construction, the redirect policy, the cache and Alt-Svc middlewares, the body and trailer paths, and the error mapping. Deep-link to the section rather than the whole spec, and leave code with no matching requirement uncommented rather than inventing a reference.
