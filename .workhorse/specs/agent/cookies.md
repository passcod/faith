---
id: COOK
---

# Cookie jar

An agent can carry a cookie jar, off by default.
With it enabled, cookies received in responses are stored and included on subsequent requests to matching URLs, giving session behaviour without the caller managing `Cookie` headers.

`cookies: true` enables the jar for the agent; the default is disabled.
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

## Expiry cap

A cookie may not persist more than 400 days.
When a cookie's expiry, taken from `Max-Age` or `Expires`, falls more than 400 days after the moment it is received, its expiry is reduced to 400 days from that moment.
A shorter expiry is left untouched, and a session cookie (one with neither attribute) stays a session cookie.

## Size and count caps

A cookie is stored only when the combined length of its name and value is at most 4096 bytes; a larger cookie is rejected.

At most 180 cookies are kept for any one host, and at most 3000 across the whole jar.
When storing a cookie would exceed either cap, the jar first discards expired cookies within that scope, then evicts the oldest remaining cookies to make room, so the incoming cookie is admitted and neither cap is ever exceeded.
