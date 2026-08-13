---
id: COOK
---

# Cookie jar

An agent can carry a cookie jar, off by default.
With it enabled, cookies received in responses are stored and included on subsequent requests to matching URLs, giving session behaviour without the caller managing `Cookie` headers.

`cookies: true` enables the jar for the agent with the default caps; the default is disabled.
`cookies` also accepts an options object, which enables the jar and tunes those caps, so `cookies: {}` means the same as `cookies: true`.
`agent.addCookie(url, cookie)` inserts a cookie; `agent.getCookie(url)` reads one back, returning `null` when there is no cookie for the URL, the jar is disabled, the URL is malformed, or the value cannot be represented as a string.
Both are silent no-ops rather than throwing on those conditions.
The jar survives `close()`: cookies remain readable from a closed agent.
With `credentials: "omit"` on a request, `Cookie` is not sent and `Set-Cookie` is stripped from the returned headers, and the jar still ingests those `Set-Cookie` values (see [REQ](../fetch/request.md)).

The jar applies the storage rules from RFC 6265bis that carry meaning without a browsing context: the `__Host-` and `__Secure-` name prefixes, a cap on how far a cookie may expire in the future, and caps on how many cookies and how many bytes a server may accumulate.
`SameSite` is not among them: it governs cross-site request behaviour that only exists in a first-party browsing context, so it has no effect here.
Every rule below governs whether a cookie is stored, so it applies identically whether the cookie arrives in a response or through `addCookie`; the request URL passed to `addCookie` supplies the scheme and host the rule reads.
A cookie a rule rejects is not stored, and the rejection is silent, consistent with the jar's other no-op behaviours.

## Name prefixes

A cookie whose name begins with `__Secure-` is stored only when it carries the `Secure` attribute and was received over a secure transport (an `https` URL); otherwise it is rejected.

A cookie whose name begins with `__Host-` is stored only when it carries the `Secure` attribute, was received over a secure transport, has no `Domain` attribute (so it is bound to the exact host that set it), and has a `Path` of `/`; otherwise it is rejected.

The prefixes are matched case-sensitively, as the standard defines them.
These rules are what the prefixes mean, so they always apply and are not tunable: a caller who wants a cookie without them names it without a prefix.

## Expiry cap

A cookie may not persist beyond the cap set by `cookies.maxAge`, in seconds, which defaults to 400 days.
When a cookie's expiry, taken from `Max-Age` or `Expires`, falls further ahead than the cap, its expiry is reduced to the cap measured from the moment the cookie is received.
A shorter expiry is left untouched, and a session cookie (one with neither attribute) stays a session cookie.

## Size and count caps

`cookies.maxSize` caps a single cookie at the combined length of its name and value in bytes, defaulting to 4096; a larger cookie is rejected.

`cookies.maxPerHost` caps how many cookies are kept for any one host, defaulting to 180, and `cookies.maxTotal` caps how many are kept across the whole jar, defaulting to 3000.
When storing a cookie would exceed either cap, the jar first discards expired cookies within that scope, then evicts the oldest remaining cookies to make room, so the incoming cookie is admitted and neither cap is ever exceeded.
A cookie counts against the domain it is stored under, which is its `Domain` attribute when it has one and the host that set it otherwise.
Domains are counted separately from each other, so a server that spreads cookies across subdomains gets `maxPerHost` in each; `maxTotal` is what bounds it in that case, and is why the jar has a whole-jar cap rather than a per-domain one alone.

The defaults follow browser practice, which is what servers are built against, and are the point of the jar rather than a hedge: a caller who needs more room raises the number.
