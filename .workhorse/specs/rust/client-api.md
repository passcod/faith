---
id: RSAPI
---

# The Rust client API

`web-faith` has one noun and one verb: an `Agent` owns the connection pool, resolver, cookie jar, HTTP cache, and HTTP/3 knowledge, and `fetch` is how every request goes out.
The surface is fetch-flavoured rather than a transcription of the JavaScript API: it keeps fetch's vocabulary and the defaults the Node surface has, and it speaks the Rust ecosystem's types wherever one exists for the job.
The agent's own behaviour, its options, and its lifecycle are specified in [AGENT](../agent/overview.md) and the specs beneath it; this spec covers the shape a Rust caller sees.

## Agent

`Agent::new()` constructs an agent with default options, and `Agent::builder()` returns a builder whose methods mirror the option groups in [AGENT](../agent/overview.md), ending in `build()`.
Nested option groups are nested builders rather than structs of optional fields, so a group left alone is absent from the call rather than spelled out as absent.
Construction validates options and reports the errors named in [AGENT](../agent/overview.md), and reads the environment variables [ENV](../environment/variables.md) names for this surface.

An agent is cheap to clone and every clone names the same underlying agent, so cloning one is how a request gets an agent to run on rather than a way to get a second pool.
Because clones share, `close()` acts on the agent itself and every handle to it sees the result.
A request that has already been issued completes, a request issued afterwards fails with the closed-agent error, and `close()` is idempotent, exactly as in [AGENT](../agent/overview.md).
The agent captures what a request needs at the moment the request is issued, which is what lets an in-flight request finish while later ones are refused.

`network_changed()`, `stats()`, `connections()`, `resolvers()`, `prefetch_dns()`, and `preconnect()` act on a live agent as their counterparts do on the Node surface.

`cookies()` reaches the agent's jar, returning the `web-faith-cookies` jar itself rather than wrapping it in per-cookie methods, so a caller inserts and reads cookies through the same type that crate documents (see [COOK](../agent/cookies.md)).
An agent with no jar has nothing to hand back and says so in the return type, which is where the Node surface's null-returning reads land in Rust.
The jar outlives `close()` and stays readable from a closed agent, as [AGENT](../agent/overview.md) requires.

`USER_AGENT` is exported so a caller can prepend its own product token to Faith's default.
The versions it embeds are not exported alongside it, a Rust caller already having its own package metadata to read them from.

## Making a request

`agent.fetch(target)` returns a fetch builder.
A target is anything that converts into a `url::Url` — a `Url`, a `&str`, a `String` — or a `Request`, or an `http::Request`.
The builder implements `IntoFuture`, so awaiting it sends the request: a bare call awaits directly, and a configured one awaits after its options.
There is no separate send step, and a builder that is dropped without being awaited sends nothing, so the builder is marked `#[must_use]`.

Builder methods cover the per-request options in [REQ](../fetch/request.md), [CANCEL](../fetch/cancellation-and-timeouts.md), [SRI](../fetch/integrity.md), [ENC](../fetch/content-encoding.md), and [CACHE](../cache/http-cache.md), and set the method, headers, and body.

`Request::new(target)` takes the same kinds of target and returns a request builder, which `build()` resolves into a `Request` rather than sending it.
A `Request` is inert, and passing one to `fetch` returns a fetch builder, so a request can be prepared once and adjusted at each call site or sent unchanged on more than one agent.
`try_clone()` copies a request when its body allows it and reports that it cannot when the body is a stream, a stream being consumable once.

The two builders carry the same option-setting methods, so a call reads the same way whichever one it is written against, and they are distinct types because their terminal steps differ.
A fetch builder is awaited and has no `build()`; a request builder is built and cannot be awaited, so an attempt to send a request that has no agent to send it on is refused by the compiler rather than surfacing when the code runs.
`try_clone()` belongs to `Request` alone, and a fetch builder is not a target, so `fetch` calls do not nest.

## Layering options

A request is built in layers: a builder wraps a target, `build()` settles it into a `Request`, and that request can itself be the target of another builder, to any depth.
Each layer wins over the layer it wraps, which is the rule the Node surface follows when options passed to `fetch()` beat the values carried on a `Request` (see [REQ](../fetch/request.md)).
So in `agent.fetch(Request::new(inner).timeout(b)).timeout(c)` the timeout is `c`, whatever `inner` set.
A setting a layer does not touch is inherited from beneath it unchanged, so wrapping a request to adjust one thing leaves the rest as it was.

Single-valued settings, among them the method, body, timeout, integrity, cache mode, credentials, priority, and request compression, take the outermost value set explicitly.
Setting one twice on a single builder is the same question at a smaller scale, and the later call wins.

Headers merge by name rather than wholesale, matching how per-request headers beat agent defaults in [REQ](../fetch/request.md).
A name an outer layer sets takes the outer value, a name only an inner layer sets is carried through, and removing a name removes what the layers beneath contributed for it.
Setting a collection of headers applies each entry as a single header would, so it adds to and overrides the set by name rather than replacing it.

The URL comes from the target at the bottom of the stack, and wrapping a request carries its URL through.

The agent is not part of this layering: a request runs on whichever agent's `fetch` was called, and a `Request` carries no agent of its own.

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
Either way it reports the error naming the offender — `InvalidHeader`, `InvalidMethod`, or `InvalidUrl` for a target that does not parse — as [REQ](../fetch/request.md) and [ERR](../errors/errors.md) require.
Holding the failure is what lets a target be given as a string: an unparseable one is reported where the request is resolved rather than by a call that cannot fail.

## What a disabled component removes

A component's Cargo feature governs the API as much as the build, so turning one off takes away the methods that only mean something with that component present: no `integrity()` without integrity, no cookie jar handle without cookies, and the same for request compression and cache mode (see [RUST](overview.md)).
Code written against a component that is not built fails to compile rather than compiling into a call that does nothing, so a build reports what it does not carry at the point the caller asks for it.

## Errors and cancellation

Failures surface as one error type whose variants are the kinds in [ERR](../errors/errors.md), each reporting the same stable code as its JavaScript counterpart.
Errors arriving from a component crate are converted into it at the boundary, so a caller matches on one type whichever layer failed.

Dropping the future cancels the request, which is how a Rust caller aborts.
The `timeout` option remains for the deadline case, and both surface the errors in [CANCEL](../fetch/cancellation-and-timeouts.md).

Asynchronous work runs on Tokio.
