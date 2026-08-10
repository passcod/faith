---
id: FAITH
---

# Fáith

Fáith is a fetch API implementation for Node.js backed by a Rust network stack instead of undici and libuv.
It is published as the native module `@passcod/faith` and aims to behave like the browser's fetch wherever that concept translates to a server-side runtime, while exposing the capabilities that stack unlocks: transparent HTTP/2 and HTTP/3, IPv4/IPv6 Happy Eyeballs, DNS caching, an optional cookie jar, and HTTP caching.

The library's contract has two halves: fidelity to the fetch standard, and divergence where the standard assumes a browser.
Divergences are deliberate, documented, and stable rather than accidental.

## Compatibility stance

`fetch(resource, options)` accepts the same shapes as WHATWG fetch: a URL string or stringifiable object (including `URL`), or a Web API `Request` object.
Behaviour follows the fetch specification by default; where browsers and the specification disagree, Fáith follows the specification (for example, `body` is `null` on responses that cannot have a body).
Browser-only concepts that assume an origin or a browsing context (CORS, `mode`, `referrer`, `referrerPolicy`, `attributionReporting`, `browsingTopics`, `keepalive`, `priority`) have no server-side meaning; passing them is harmless and they take no effect.
Options in a `RequestInit` that Fáith does not recognise are ignored rather than rejected.
Fáith-specific extensions (such as `agent`, `timeout`, response `peer`, `version`, `trailers`, `discard()`) are additive: code written against standard fetch runs unmodified.

## Protocol support

Requests negotiate HTTP/1.1 or HTTP/2 over ALPN transparently.
HTTP/3 is reached through the Alt-Svc upgrade mechanism (see [H3UP](http3/upgrade.md)) or through explicit hints, never by breaking a request that could have succeeded over TCP.
IPv4 and IPv6 are both used, racing connections with the Happy Eyeballs algorithm when Fáith's own DNS client is in use (see [DNS](agent/dns.md)).

## Packaging

The package ships prebuilt native binaries per platform; installing it does not require a Rust toolchain.
TypeScript typings cover the full public API, including Fáith-specific extensions and the `ERROR_CODES` map.
Two version constants are exported: `FAITH_VERSION` (the library itself) and `REQWEST_VERSION` (the underlying HTTP stack), usable in user agent strings and diagnostics.
