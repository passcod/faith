# Cards spun off from serving stale cache entries

Work that came up while speccing RFC 5861 support but sits outside it.

## Specify request-side cache directives

Faith expresses request cache intent through the `cache` mode enum, and never reads or writes the request's `Cache-Control` header itself.
A caller-set `Cache-Control` request header therefore passes straight through to the caching layer, which already honours `no-cache`, `pragma: no-cache`, `max-age`, `min-fresh`, and `max-stale` against the stored entry.
That behaviour works today and no spec describes it, so a caller cannot tell whether it is supported or incidental, and how it composes with the `cache` mode when the two disagree is undefined.
This card settles what Faith promises for request directives, including the request form of `stale-if-error` (RFC 5861), which is not honoured and would need the same window check the response directive uses.
