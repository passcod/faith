---
id: REDIR
---

# Redirects

Redirect handling is configured on the agent, not per request: the `redirect` option in a `RequestInit` is not respected, and the agent's policy applies to every request it executes.
This keeps redirect behaviour a property of the client configuration rather than something that varies call by call.

## Policies

`follow` (the default) follows redirects automatically, up to 10 hops; exceeding the limit is a network error carrying `Network` (see [ERR](../errors/errors.md)).
`error` rejects the fetch promise when a redirect status is returned, with a network error carrying `Redirect` so a caller can tell a refused redirect from a transport failure.
`stop` (Faith-specific) follows nothing and returns the redirect response itself, giving callers manual control without the browser's opaque `manual` semantics.
`manual` is accepted for compatibility with the fetch standard's vocabulary and behaves as `follow`.

## Method and body while following

A 301, 302, or 303 turns the redirected request into a `GET`, dropping the request body and its `Content-Type`.
A 307 or 308 preserves the method and replays the body.
A request whose body is a `ReadableStream` cannot be replayed, so a 307 or 308 is not followed and the redirect response is returned to the caller as if the policy were `stop`.

## Headers while following

`Authorization`, `Cookie`, and `Proxy-Authorization` are removed when the redirect target differs from the previous URL in host, port, or scheme, and are carried through when all three match.
The `Referer` header is set to the previous URL, stripped of any userinfo and fragment, except when the redirect steps down from `https` to `http`.

## What the response reports

`response.url` reports the final URL after all redirects, and `response.version` the final protocol version.
`response.redirected` is true when the response resulted from following at least one redirect.
With the agent's `quirks.h3FollowAdvertisedPort` enabled, URL comparison for this flag ignores the port (the port is rewritten by the upgrade), so a genuine redirect differing only in port reads as not-redirected on those responses (see [H3UP](../http3/upgrade.md)).
