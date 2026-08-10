# Prune or wire up the dead error codes

## Why the two named codes get pruned rather than wired

`ResponseBodyNotAvailable` has no home. A response that cannot carry a body has `body` set to `null`, which is the specified behaviour rather than a failure (see [BODY](../../specs/response/reading-the-body.md)), so there is no path that would want to throw a not-available error.

`RuntimeThread` has no home either. Fáith does not start or own a tokio runtime: futures go onto napi's shared runtime via `faith_promise`. The one place a runtime handle might be missing is the body drop path during garbage collection, and the behaviour there is to close the connection instead of returning it to the pool, not to raise an error.

There is precedent for the prune: commit 84201ba removed a batch of unused variants and left these two behind.

## The `Redirect` code needs a decision

`Redirect` is constructed in the agent's redirect policy but never reaches a caller: reqwest wraps it as its own redirect error, and `From<reqwest::Error>` flattens everything that is not a timeout to `Network`. So it fails the reachability rule in [ERR](../../specs/errors/errors.md) the same way the two named codes do, even though it does have a construction site.

Two ways out, and they are not equivalent:

- Wire it up by branching on `err.is_redirect()` in `From<reqwest::Error>`. This also catches the hop-limit case, because reqwest gives exceeding the limit the same error kind as a policy refusal, so [REDIR](../../specs/fetch/redirects.md) would need to say which code each of those two carries.
- Prune it alongside the other two, leaving a refused redirect as a `Network` error with the reason in the message, which is what callers see today.

## Steps

- [ ] Resolve the `Redirect` question and fold the answer into ERR's mapping (and REDIR, if it gets wired)
- [ ] Remove the pruned variants from `FaithErrorKind` and its `default_message` and `js_type` arms
- [ ] Bring the `error.rs` doc comment's mapping table in line with the enum, adding `AddressParse`, `IntegrityMismatch`, and `InvalidIntegrity`
- [ ] Regenerate `index.d.ts` and update the matching table in `README.md` and the `ERROR_CODES` list in `wrapper.d.ts`
- [ ] Check the test suite for references to the pruned codes
