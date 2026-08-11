# Prune or wire up the dead error codes

Three codes were unreachable, not two. The two the card names get pruned; `Redirect` gets wired up.

## Why the two named codes get pruned rather than wired

`ResponseBodyNotAvailable` has no home. A response that cannot carry a body has `body` set to `null`, which is the specified behaviour rather than a failure (see [BODY](../../specs/response/reading-the-body.md)), so there is no path that would want to throw a not-available error.

`RuntimeThread` has no home either. Fáith does not start or own a tokio runtime: futures go onto napi's shared runtime via `faith_promise`. The one place a runtime handle might be missing is the body drop path during garbage collection, and the behaviour there is to close the connection instead of returning it to the pool, not to raise an error.

There is precedent for the prune: commit 84201ba removed a batch of unused variants and left these two behind.

## Why `Redirect` was unreachable, and how it is wired

`Redirect` has a construction site, in the agent's `error` redirect policy, so it passes the card's "no construction site" test. It was still unreachable as a `code`: reqwest wraps the policy error in one of its own, and `From<reqwest::Error>` mapped everything that was not a timeout to `Network`.

The conversion now recovers the kind by walking the source chain for a `FaithError`. Redirect failures reqwest raises itself, exhausting the hop limit or an https-only downgrade, carry no `FaithError` and stay `Network`, which is what keeps the two distinguishable. The alternative, branching on `is_redirect()` alone, would have given the hop limit the `Redirect` code too, since reqwest gives both cases the same error kind.

One trap worth remembering: the policy has to hand reqwest the `FaithError` unboxed. `Box::new(FaithError)` lands a `Box<FaithError>` in the source chain, which does not `downcast_ref` back to `FaithError`, and its `Debug` output is identical, so the failure looks like the error simply not being there.

## Steps

- [x] Wire `Redirect` through `From<reqwest::Error>` and fold it into ERR's mapping and REDIR's policy list
- [x] Remove the pruned variants from `FaithErrorKind` and its `default_message` and `js_type` arms
- [x] Bring the `error.rs` doc comment's mapping table in line with the enum, adding `AddressParse`, `IntegrityMismatch`, and `InvalidIntegrity`
- [x] Regenerate `index.d.ts` and update the matching table in `README.md` and the `ERROR_CODES` list in `wrapper.d.ts`
- [x] Assert the codes in the redirect tests, which previously only checked that something was thrown
