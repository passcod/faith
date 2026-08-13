# Cookie jar: RFC 6265bis rules

Scenarios verifying the storage rules the jar enforces, and the `cookies` options that tune them.
Rust unit tests live in `src/cookies.rs`; JS tests in `test/cookies.test.js` run against local httpbin.

## Name prefixes

- [x] A `__Host-` cookie with `Secure`, no `Domain`, and `Path=/` over https is stored (verifies spec: COOK)
- [x] A `__Host-` cookie without the `Secure` attribute is rejected (verifies spec: COOK)
- [x] A `__Host-` cookie received over http is rejected (verifies spec: COOK)
- [x] A `__Host-` cookie carrying a `Domain` attribute is rejected (verifies spec: COOK)
- [x] A `__Host-` cookie with a non-root `Path` is rejected (verifies spec: COOK)
- [x] A `__Host-` cookie with no `Path` attribute at all is rejected (verifies spec: COOK)
- [x] A `__Secure-` cookie with `Secure` over https is stored, and may carry `Domain` and `Path` (verifies spec: COOK)
- [x] A `__Secure-` cookie without the `Secure` attribute is rejected (verifies spec: COOK)
- [x] A `__Secure-` cookie received over http is rejected (verifies spec: COOK)
- [x] Names differing only in case (`__host-`, `__SECURE-`) carry no prefix requirement (verifies spec: COOK)
- [x] An unprefixed cookie is unaffected by the prefix rules (verifies spec: COOK)
- [ ] A prefixed cookie arriving as `Set-Cookie` over a real https connection is gated the same way as one added through `addCookie`

## Expiry cap

- [x] A `Max-Age` beyond the cap is reduced to the cap (verifies spec: COOK)
- [x] An `Expires` beyond the cap is reduced to the cap (verifies spec: COOK)
- [x] An expiry shorter than the cap is left untouched (verifies spec: COOK)
- [x] A cookie with neither attribute stays a session cookie (verifies spec: COOK)
- [x] `Max-Age` takes precedence over `Expires`, so a short `Max-Age` beside a far-future `Expires` wins (verifies spec: COOK)
- [x] A server can still delete a cookie by resending it already expired (verifies spec: COOK)
- [x] `cookies.maxAge` sets the cap, and a cookie asking for longer dies at it (verifies spec: COOK)

## Size cap

- [x] A cookie whose name and value exceed the cap is not stored (verifies spec: COOK)
- [x] A cookie exactly at the cap is stored (verifies spec: COOK)
- [x] The cap counts name and value together, not either alone (verifies spec: COOK)
- [x] Attributes do not count towards the cap (verifies spec: COOK)
- [x] `cookies.maxSize` sets the cap (verifies spec: COOK)

## Count caps

- [x] Exceeding the per-domain cap evicts the oldest cookie for that domain (verifies spec: COOK)
- [x] Each domain gets its own per-domain allowance (verifies spec: COOK)
- [x] Rewriting an existing cookie keeps its place in the order, so a refreshed session cookie does not evict the rest (verifies spec: COOK)
- [x] Expired cookies are discarded before live ones are evicted (verifies spec: COOK)
- [x] The whole-jar cap bounds cookies spread across many domains (verifies spec: COOK)
- [x] A cap of zero stores nothing (verifies spec: COOK)
- [x] A cookie carrying a `Domain` attribute is evictable, its key matching the one it was stored under (verifies spec: COOK)
- [x] Cookies arriving as `Set-Cookie` from a server are subject to the count caps (verifies spec: COOK)
- [x] `cookies.maxPerHost` and `cookies.maxTotal` set the caps (verifies spec: COOK)
- [ ] Eviction under concurrent requests to the same host leaves the jar within its caps and never panics

## Options and existing behaviour

- [x] `cookies: {}` enables the jar, the same as `cookies: true` (verifies spec: COOK)
- [x] `cookies: true` still enables the jar with the default caps (verifies spec: COOK)
- [x] `cookies: false` and an absent option leave the jar disabled, with `getCookie` returning null (verifies spec: COOK)
- [x] Cookies are still scoped to their domain, and a rejected cookie leaves the jar unchanged (verifies spec: COOK)
- [x] A cookie that does not parse is dropped silently (verifies spec: COOK)
- [x] Cookies still persist across requests and stay separate between agents (verifies spec: COOK)
- [ ] With `credentials: "omit"`, a `Set-Cookie` the jar would reject is not stored, while an acceptable one still is
