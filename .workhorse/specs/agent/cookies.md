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
