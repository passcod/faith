# Modernise the cookie jar for RFC 6265bis

Implements [COOK](../../specs/agent/cookies.md): name prefixes, expiry cap, size and count caps, and the `cookies` options object.

## Where the work lands

reqwest's `Jar` wraps `RwLock<cookie_store::CookieStore>` in a private field, so it cannot be extended from outside.
`cookie_store` 0.22 enforces classic RFC 6265 storage (http-only scheme, public suffix, domain match, expiry) and none of the bis rules, so fáith supplies its own store implementing `reqwest::cookie::CookieStore` and wrapping `cookie_store::CookieStore` directly, keeping the classic rules and gating on the bis ones.

Eviction needs a notion of "oldest", which `cookie_store::Cookie` does not carry: it has no creation or last-access time.
The store therefore keeps its own insertion-order map keyed by the same `(domain, path, name)` triple `cookie_store` keys by, so an evicted key can be handed straight to `CookieStore::remove`.

Cookies are gated on the way in rather than filtered on the way out, so a rejected cookie never occupies space and the caps bound real memory.
`addCookie` and `Set-Cookie` share that one path, which is what makes the rules apply identically to both.

## Steps

- [x] Add `cookie`, `cookie_store`, and `time` as direct dependencies, pinned to the versions reqwest already resolves
- [x] New `src/cookies.rs`: `CookieLimits` (with the spec's defaults) and `FaithJar`
- [x] Prefix rules: `__Secure-` and `__Host-` gates, case-sensitive
- [x] Expiry clamp, following the storage model's `Max-Age`-over-`Expires` precedence
- [x] Size cap on name plus value in bytes
- [x] Count caps: purge expired within scope, then evict oldest, per domain then whole jar
- [x] Implement `reqwest::cookie::CookieStore` (`set_cookies`, `cookies`) and `add_cookie_str`
- [x] `AgentCookieOptions` napi object; `cookies` accepts `Either<bool, AgentCookieOptions>`
- [x] Wire the jar into `with_options_inner`, and point `addCookie`/`getCookie` at it
- [x] Rust unit tests for each rule; JS tests for the option plumbing
- [x] Run `cargo test`, `cargo clippy`, and the JS suite against local httpbin

## Notes

`SameSite` is parsed by the `cookie` crate and carried through untouched; it governs cross-site behaviour that needs a browsing context, so the store does not read it.

The secure-transport test is `https` exactly, per the spec.
Browsers additionally treat `http://localhost` as trustworthy, so a `__Host-` cookie that works in a browser against a local dev server is rejected here.
Worth revisiting if it bites; it is a spec change, not a bug fix.
