---
id: RSAPI
---

# The Rust client API

`web-faith` has one noun and one verb: an `Agent` owns the connection pool, resolver, cookie jar, HTTP cache, and HTTP/3 knowledge, and `fetch` is how every request goes out.
The surface is fetch-flavoured rather than a transcription of the JavaScript API: it keeps fetch's vocabulary and its browser-like defaults, and it speaks the Rust ecosystem's types wherever one exists for the job.
The agent's own behaviour, its options, and its lifecycle are specified in [AGENT](../agent/overview.md) and the specs beneath it; this spec covers the shape a Rust caller sees.

## Agent

`Agent::new()` constructs an agent with default options, and `Agent::builder()` returns a builder whose methods mirror the option groups in [AGENT](../agent/overview.md), ending in `build()`.
Nested option groups are nested builders rather than structs of optional fields, so a group left alone is absent from the call rather than spelled out as absent.
Construction validates options and reports the errors named in [AGENT](../agent/overview.md).

An agent is cheap to clone and every clone names the same underlying agent, so cloning one is how a request gets an agent to run on rather than a way to get a second pool.
Because clones share, `close()` acts on the agent itself and every handle to it sees the result.
A request that has already been issued completes, a request issued afterwards fails with the closed-agent error, and `close()` is idempotent, exactly as in [AGENT](../agent/overview.md).
The agent captures what a request needs at the moment the request is issued, which is what lets an in-flight request finish while later ones are refused.

`network_changed()`, `stats()`, `connections()`, `resolvers()`, `prefetch_dns()`, and `preconnect()` act on a live agent as their counterparts do on the Node surface.

## Making a request

`agent.fetch(target)` returns a request builder, where `target` is a URL, a `Request`, or an `http::Request`.
The builder implements `IntoFuture`, so awaiting it sends the request: a bare call awaits directly, and a configured one awaits after its options.
There is no separate send step, and a builder that is dropped without being awaited sends nothing, so the builder is marked `#[must_use]`.

Builder methods cover the per-request options in [REQ](../fetch/request.md), [CANCEL](../fetch/cancellation-and-timeouts.md), [SRI](../fetch/integrity.md), [ENC](../fetch/content-encoding.md), and [CACHE](../cache/http-cache.md), and set the method, headers, and body.

`Request::new(target)` takes the same three kinds of target and returns the same builder, which `build()` resolves into a `Request` rather than sending it.
A `Request` is inert, and passing one to `fetch` returns a builder again, so a request can be prepared once and adjusted at each call site or sent unchanged on more than one agent.
`try_clone()` copies a request when its body allows it and reports that it cannot when the body is a stream, a stream being consumable once.

A builder that carries no agent cannot be awaited, and the compiler refuses it rather than the call failing when it runs.

## Reading a response

A `Response` carries `status`, `status_text`, `ok`, `headers`, `url`, `redirected`, `kind`, and `body_used` with the meanings in [RESP](../response/response.md), and Faith's own `peer` and `version`.
`text()`, `json()`, `bytes()`, `body_stream()`, `to_file()`, and `discard()` read the body, and `trailers()` and `timing()` resolve as in [TRL](../response/trailers.md) and [RESP](../response/response.md).
Reading the body consumes it and a second read fails, following the fetch standard rather than the owned-response model of other Rust clients, as in [BODY](../response/reading-the-body.md).

The response body implements `http_body::Body`, and a `Response` converts into an `http::Response`, so a Faith response feeds code written against the wider ecosystem without a shim.

## Ecosystem types

Types from `http` are canonical wherever one exists: `Method`, `HeaderName`, `HeaderValue`, `HeaderMap`, `StatusCode`, and `Version`.
URLs are `url::Url`, which is what the fetch standard's parsing rules describe and what the stack beneath already uses.
`status_text` and `ok` derive from the status code rather than being carried separately.

Setters accept anything that converts into the canonical type, so a string literal works where JavaScript would pass a string and a typed value works where a caller already holds one.
A conversion that fails is held until the builder resolves and surfaces there: at `build()` for a request, and at the await for a fetch.
Either way it reports the invalid-header or invalid-method error naming the offender, as [REQ](../fetch/request.md) requires.

## Errors and cancellation

Failures surface as one error type whose variants are the kinds in [ERR](../errors/errors.md), each reporting the same stable code as its JavaScript counterpart.
Errors arriving from a component crate are converted into it at the boundary, so a caller matches on one type whichever layer failed.

Dropping the future cancels the request, which is how a Rust caller aborts.
The `timeout` option remains for the deadline case, and both surface the errors in [CANCEL](../fetch/cancellation-and-timeouts.md).

Asynchronous work runs on Tokio.
