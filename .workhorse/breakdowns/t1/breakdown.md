# Follow-ups from the state-of-Fáith audit

Issues surfaced while speccing the current state of Fáith on this card. Each was verified against a built module before being listed; this card changes no code, so the fixes land as their own cards.

## Memoise the body stream in the wrapper

The wrapper's `body` getter calls the native `body()` on every access, minting a fresh stream each time, and a handle taken after partial consumption replays the full body from the start. That is a same-response double read with no `clone()`, which the once-only model forbids (spec BODY). Memoise the `ReadableStream` in the getter the way `headers` is memoised, giving the standard's same-object semantics and making the replay path unreachable from the public API. Callers requiring the raw binding directly would still get replaying handles; decide whether that needs closing too.

## Surface the Redirect error code

With `redirect: "error"`, the rejection arrives as `name: NetworkError, code: Network`. The redirect policy constructs the `Redirect` kind, but it round-trips through a `reqwest::Error` and the conversion re-maps it to `Network`, so the `Redirect` code documented in the README and spec ERR never reaches JS. Unwrap the inner FaithError (it is visible in the error's source chain) so the code survives the trip.

## Prune or wire up the dead error codes

`ResponseBodyNotAvailable` and `RuntimeThread` have no construction site anywhere in `src/` yet are exported in `ERROR_CODES`. Decide whether to remove them from the enum or wire them where they were meant to fire. While in there, the mapping table in the `error.rs` doc comment omits `AddressParse`, `IntegrityMismatch`, and `InvalidIntegrity`; bring it in line with the enum.

## Decide the HTTP/3 attempt-timeout retry semantics

When `upgradeAttemptTimeout` expires, the cloned request is retried over TCP regardless of method. A timed-out attempt may nonetheless have been delivered, so a non-idempotent request (POST) can be executed twice. Decide whether to bless that as documented behaviour or to guard it, for example by only auto-retrying idempotent methods and surfacing an error for the rest, then update spec H3UP to record the rule.
