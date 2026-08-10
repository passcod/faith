---
id: REQ
---

# Making a request

`fetch(resource, options)` builds and executes an HTTP request through an agent. Every request runs on an agent: one passed via the `agent` option, or a shared global default agent constructed with no options on first use.

## Resource

- [ ] A string, `URL`, or any object with a stringifier names the target. The URL must be absolute, with a scheme; an unparseable URL throws an invalid-URL error.
- [ ] A Web API `Request` object is accepted and converted to options: its fields are copied, then any options passed directly to `fetch()` win over the `Request`'s own values.
- [ ] A `Request` with a body has that body read to completion during conversion; Fáith-specific options do not survive a trip through `Request`, so callers wanting streaming uploads or custom options supply them directly to `fetch()`.

## Method and headers

- [ ] The method defaults to `GET`, is uppercased, and an invalid method throws an invalid-method error.
- [ ] Headers are accepted as a `Headers` object or a plain object literal; other shapes throw. A header set to `null` is removed.
- [ ] All request headers can be set; Fáith enforces no browser forbidden-header list.
- [ ] A per-request header with an invalid name or value throws an invalid-header error naming the offender. (Agent-level default headers are instead dropped silently: a construction-time convenience versus a per-call mistake.)
- [ ] Per-request headers override the agent's default headers on a per-name basis.

## Body

- [ ] Accepted body types: string, `ArrayBuffer`, `Blob`, `DataView`, `File`, `FormData`, `TypedArray`, `URLSearchParams`, and `ReadableStream`.
- [ ] A `URLSearchParams` body sets `Content-Type: application/x-www-form-urlencoded;charset=UTF-8` when no content type was given.
- [ ] A `ReadableStream` body requires `duplex: "half"`, matching the fetch standard; the stream is sent as the request body without buffering the whole payload.
- [ ] Fáith operates in half duplex: the whole request is sent before the response is processed.

## Credentials

- [ ] `credentials` defaults to `include` (there being no origin to be same as, `same-origin` is accepted and treated as `include`).
- [ ] `credentials: "omit"` strips username/password from the URL, removes any `Cookie` header from the request, and removes `Set-Cookie` from the headers returned to the caller.
- [ ] Two upstream limitations hold: an agent-configured TLS client certificate is still presented under `omit`, and an agent cookie jar still ingests `Set-Cookie` values even as they are stripped from the visible headers (see `agent/cookies.md`).

## Options without effect

- [ ] Options that assume a browser (`mode`, `referrer`, `referrerPolicy`, `attributionReporting`, `browsingTopics`, `keepalive`, `priority`) and options Fáith does not recognise are ignored rather than rejected.
- [ ] `redirect` on the request is ignored; redirect policy lives on the agent (see `fetch/redirects.md`).

## Related request options

- [ ] `cache` selects the HTTP cache mode for this request (see `cache/http-cache.md`).
- [ ] `integrity` requests body verification (see `fetch/integrity.md`).
- [ ] `signal` and `timeout` cancel the request (see `fetch/cancellation-and-timeouts.md`).
