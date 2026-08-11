---
id: REQ
---

# Making a request

`fetch(resource, options)` builds and executes an HTTP request through an agent.
Every request runs on an agent: one passed via the `agent` option, or a shared global default agent constructed with no options on first use.

## Resource

A string, `URL`, or any object with a stringifier names the target.
The URL must be absolute, with a scheme; an unparseable URL throws an invalid-URL error.
A Web API `Request` object is accepted and converted to options: its fields are copied, then any options passed directly to `fetch()` win over the `Request`'s own values.
A `Request` with a body has that body read to completion during conversion; Fáith-specific options do not survive a trip through `Request`, so callers wanting streaming uploads or custom options supply them directly to `fetch()`.

## Method and headers

The method defaults to `GET` and an invalid method throws an invalid-method error.
A method that matches `DELETE`, `GET`, `HEAD`, `OPTIONS`, `POST`, or `PUT` case-insensitively is normalised to upper case, matching the set the fetch standard normalises; any other method is sent with its case as given, so a server routing case-sensitively on a custom method sees the method the caller wrote.
Headers are accepted as a `Headers` object or a plain object literal; other shapes throw.
A header set to `null` is removed.
All request headers can be set; Fáith enforces no browser forbidden-header list.
A per-request header with an invalid name or value throws an invalid-header error naming the offender, while agent-level default headers with an invalid name or value are dropped (see [AGENT](../agent/overview.md)).
Per-request headers override the agent's default headers on a per-name basis.

## Headers Fáith sets

A request the caller adds no headers to still carries `Host` for the target authority, `Accept: */*`, `Accept-Encoding` advertising the encodings Fáith can decode (see [ENC](content-encoding.md)), and `User-Agent` (see [AGENT](../agent/overview.md)).
A caller-supplied or agent-default value for any of these replaces the value Fáith would otherwise send.
An `Accept-Encoding` from the caller also selects which codings Fáith decodes on the way back (see [ENC](content-encoding.md)).

## Request priority

The `priority` option carries a hint about how the request ranks against others, expressed on the wire as an RFC 9218 `Priority` header with an urgency (`u`) parameter. Urgency runs from 0 (most urgent) to 7 (least urgent), and lower urgency means the server should serve the request sooner.

`priority: "high"` sends a low urgency value, marking the request as more urgent than a default request.
`priority: "low"` sends a high urgency value, marking it less urgent than a default request.
`priority: "auto"`, an unrecognised value, and the absence of the option all send no `Priority` header, leaving the request at the server's default urgency.

A `Priority` header the caller sets directly, or one configured as an agent default header, wins over the mapping: Fáith emits the caller's header unchanged and does not derive a value from `priority`.

## Body

Accepted body types: string, `ArrayBuffer`, `Blob`, `DataView`, `File`, `FormData`, `TypedArray`, `URLSearchParams`, and `ReadableStream`.
A `URLSearchParams` body sets `Content-Type: application/x-www-form-urlencoded;charset=UTF-8` when no content type was given.
A `ReadableStream` body requires `duplex: "half"`, matching the fetch standard; the stream is sent as the request body without buffering the whole payload.
Fáith operates in half duplex: the whole request is sent before the response is processed.

## Credentials

`credentials` defaults to `include` (there being no origin to be same as, `same-origin` is accepted and treated as `include`).
`credentials: "omit"` strips username/password from the URL, removes any `Cookie` header from the request, and removes `Set-Cookie` from the headers returned to the caller.
An agent-configured TLS client certificate is still presented under `omit`, and an agent cookie jar still ingests `Set-Cookie` values even as they are stripped from the headers the caller sees (see [COOK](../agent/cookies.md)).

## Options without effect

Options that assume a browser (`mode`, `referrer`, `referrerPolicy`, `attributionReporting`, `browsingTopics`, `keepalive`) and options Fáith does not recognise are ignored rather than rejected.
`redirect` on the request is ignored; redirect policy lives on the agent (see [REDIR](redirects.md)).

## Related request options

`cache` selects the HTTP cache mode for this request (see [CACHE](../cache/http-cache.md)).
`integrity` requests body verification (see [SRI](integrity.md)).
`signal` and `timeout` cancel the request (see [CANCEL](cancellation-and-timeouts.md)).
Compressed transfer needs no option and is described in [ENC](content-encoding.md).
