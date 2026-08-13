---
id: FAITH
---

# Faith

Faith is a fetch API implementation for Node.js backed by a Rust network stack rather than Node's built-in HTTP machinery.
It is published as the native module `@passcod/faith` and aims to behave like the browser's fetch wherever that concept translates to a server-side runtime, while exposing the capabilities that stack unlocks: transparent HTTP/2 and HTTP/3, IPv4/IPv6 Happy Eyeballs, DNS caching, an optional cookie jar, and HTTP caching.

The library's contract has two halves: fidelity to the fetch standard, and divergence where the standard assumes a browser.

## The standards Faith answers to

The [WHATWG Fetch standard](https://fetch.spec.whatwg.org/) defines the API surface, and the [trailers proposal](https://github.com/whatwg/fetch/issues/1940) defines `response.trailers` (see [TRL](response/trailers.md)).
[Subresource Integrity](https://www.w3.org/TR/SRI/) defines the `integrity` option (see [SRI](fetch/integrity.md)).
On the wire, Faith answers to HTTP semantics and caching (RFC 9110 and RFC 9111), HTTP/1.1 (RFC 9112), HTTP/2 (RFC 9113), HTTP/3 (RFC 9114), Alt-Svc (RFC 7838), cookies (RFC 6265), and Happy Eyeballs (RFC 8305).

Each of these is the reference against which Faith's behaviour is judged, and behaviour that departs from one is named as a divergence in the spec that covers it.
A divergence is either a browser concept with no server-side meaning or a choice Faith makes knowingly; anything else that departs from a standard is a defect in Faith rather than a licence to rewrite the spec around it.

## Compatibility stance

`fetch(resource, options)` accepts the same shapes as WHATWG fetch: a URL string or stringifiable object (including `URL`), or a Web API `Request` object.
Behaviour follows the fetch specification by default; where browsers and the specification disagree, Faith follows the specification (for example, `body` is `null` on responses that cannot have a body).
Browser-only concepts that assume an origin or a browsing context (CORS, `mode`, `referrer`, `referrerPolicy`, `attributionReporting`, `browsingTopics`, `keepalive`) have no server-side meaning; passing them is harmless and they take no effect.
Options in a `RequestInit` that Faith does not recognise are ignored rather than rejected.
Faith-specific extensions (such as `agent`, `timeout`, response `peer`, `version`, `trailers`, `discard()`) are additive: code written against standard fetch runs unmodified.

## Protocol support

Requests negotiate HTTP/1.1 or HTTP/2 over ALPN transparently.
HTTP/3 is reached through the Alt-Svc upgrade mechanism (see [H3UP](http3/upgrade.md)) or through explicit hints, never by breaking a request that could have succeeded over TCP.
IPv4 and IPv6 are both used, racing connections with the Happy Eyeballs algorithm when Faith's own DNS client is in use (see [DNS](agent/dns.md)).
