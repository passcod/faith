---
id: COOK
---

# Cookie jar

An agent can carry a cookie jar, off by default.
With it enabled, cookies received in responses are stored and included on subsequent requests to matching URLs, giving session behaviour without the caller managing `Cookie` headers.

The jar is enabled per agent through the cookies option, disabled by default: enabling it takes either a bare on switch, which uses the default limits, or an options object that enables it and tunes those limits, the empty object meaning the same as the bare switch.
A cookie is inserted against a URL and read back against one; a read yields nothing when there is no cookie for the URL, the jar is disabled, the URL is malformed, or the value cannot be represented, and a read is a silent no-op rather than throwing on those conditions.
The two surfaces spell these differently: the Node surface has per-cookie agent methods, while a Rust caller reaches the jar the agent hands back and inserts and reads through it (see [RSAPI](../rust/client-api.md)).
The jar survives `close()`: cookies remain readable from a closed agent.
With `credentials: "omit"` on a request, `Cookie` is not sent and `Set-Cookie` is stripped from the returned headers, and the jar still ingests those `Set-Cookie` values (see [REQ](../fetch/request.md)).

The jar applies the storage rules from RFC 6265bis that carry meaning without a browsing context: the `__Host-` and `__Secure-` name prefixes, a limit on how far a cookie may expire in the future, and limits on how many cookies and how many bytes a server may accumulate.
`SameSite` is not among them: it governs cross-site request behaviour that only exists in a first-party browsing context, so it has no effect here.
Every rule below governs whether a cookie is stored, so it applies identically whether the cookie arrives in a response or through a direct insert; the URL an insert is made against supplies the scheme and host the rule reads.
A cookie a rule rejects is not stored.
The Node surface takes an insert as a no-op, so a rejection there is silent, consistent with its other no-op behaviours; a Rust caller inserting a cookie directly is told which rule refused it (see [RSAPI](../rust/client-api.md)).

## Name prefixes

A cookie whose name begins with `__Secure-` is stored only when it carries the `Secure` attribute and was received over a secure transport (an `https` URL); otherwise it is rejected.

A cookie whose name begins with `__Host-` is stored only when it carries the `Secure` attribute, was received over a secure transport, has no `Domain` attribute (so it is bound to the exact host that set it), and has a `Path` of `/`; otherwise it is rejected.

The prefixes are matched case-sensitively, as the standard defines them.
These rules are what the prefixes mean, so they always apply and are not tunable: a caller who wants a cookie without them names it without a prefix.

## Expiry limit

A cookie may not persist beyond the expiry limit, in seconds, which defaults to 400 days.
When a cookie's expiry, taken from `Max-Age` or `Expires`, falls further ahead than the limit, its expiry is reduced to the limit measured from the moment the cookie is received.
A shorter expiry is left untouched, and a session cookie (one with neither attribute) stays a session cookie.

## Size and count limits

The per-cookie size limit applies to the combined length of a cookie's name and value in bytes, defaulting to 4096; a larger cookie is rejected.

The per-host count limit is how many cookies are kept for any one host, defaulting to 180, and the whole-jar count limit is how many are kept across the whole jar, defaulting to 3000.
When storing a cookie would exceed either limit, the jar first discards expired cookies within that scope, then evicts the oldest remaining cookies to make room, so the incoming cookie is admitted and neither limit is ever exceeded.
A cookie counts against the domain it is stored under, which is its `Domain` attribute when it has one and the host that set it otherwise.
Domains are counted separately from each other, so a server that spreads cookies across subdomains gets the per-host allowance in each; the whole-jar limit is what bounds it in that case, and is why the jar has a whole-jar limit rather than a per-domain one alone.

The defaults follow browser practice, which is what servers are built against, and are the point of the jar rather than a hedge: a caller who needs more room raises the number.
