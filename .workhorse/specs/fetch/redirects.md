---
id: REDIR
---

# Redirects

Redirect handling is configured on the agent, not per request: the `redirect` option in a `RequestInit` is not respected, and the agent's policy applies to every request it executes.
This keeps redirect behaviour a property of the client configuration rather than something that varies call by call.

## Policies

`follow` (the default) follows redirects automatically, up to 10 hops; exceeding the limit is a network error.
`error` rejects the fetch promise with a network error when a redirect status is returned.
`stop` (Fáith-specific) follows nothing and returns the redirect response itself, giving callers manual control without the browser's opaque `manual` semantics.

## Behaviour while following

The `Referer` header is set on redirected requests.
`response.url` reports the final URL after all redirects, and `response.version` the final protocol version.
`response.redirected` is true when the response resulted from following at least one redirect.
With the agent's `http3.upgradeFollowAdvertisedPort` enabled, URL comparison for this flag ignores the port (the port is rewritten by the upgrade), so a genuine redirect differing only in port reads as not-redirected on those responses (see [H3UP](../http3/upgrade.md)).
