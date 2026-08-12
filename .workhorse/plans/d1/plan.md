# Preconnect and DNS prefetch

Working notes for D1. The behaviour is specified in [WARM](../../specs/agent/warm-up.md); this file holds the technical constraints the spec review turned up.

## The pool cannot be populated without a request

`hyper-util`'s pool insert (`Pool::put`) is private, its legacy pool module is not reachable from `reqwest`'s public API, and `reqwest` 0.13.4 exposes no preconnect or pool-warming call. So a TCP warm-up has to send a real request, which is why `preconnect` is specified as sending a synthetic `HEAD` to the origin root. `H3Prober` in `src/alt_svc.rs` already works this way and is the model to follow.

If a future `reqwest` gains a pool-warming API, the synthetic request becomes an implementation choice again rather than a constraint, and the spec's "what preconnect sends" section is what would need revisiting. This is recorded in [the upstream limitations register](../../upstream-limitations.md), which is where such a stack upgrade gets checked against.

## Interaction with the dead-connection retry

F1 landed a retry for requests written into a pooled connection the origin had silently closed, and it deliberately excludes `POST`, `PATCH`, and `ReadableStream` bodies. A warm-up creates a pooled connection that would not otherwise exist and leaves it idle longer than a just-used one, so it raises the odds of meeting that excluded case. The spec now states the trade rather than promising a warm-up can never cost a request, and `src/retry.rs` is the code a warm-up connection has to fall through correctly.

The `aggressive idle close` conformance dimension F1 added (`test/conformance/dimensions/idle-close.js`) is the natural place to prove a warm-up connection recovers for a retryable method, and fails cleanly for a `POST`.

## Cache bypass and probe triggering pull in opposite directions

The H3 probe sends its `HEAD` on the **raw** `reqwest::Client` rather than the middleware stack, which is what gets it the HTTP cache bypass and stops probes recursing. A warm-up needs the same cache bypass, but the spec also has a TCP warm-up to a probe-worthy origin trigger a background HTTP/3 probe, and that trigger lives in `AltSvcMiddleware`. Copying the probe's raw-client approach wholesale would satisfy the cache requirement and silently drop the probe trigger.

So the warm-up needs one of: the middleware stack with the cache layer disabled for this request, or the raw client plus an explicit probe trigger on the way past. Worth deciding before writing the TCP path, because it is easy to get the cache half right and not notice the probe half is missing.

## Timeouts bound the warm-up, and it never rejects

The promise resolves and never rejects, so every failure path (DNS, refused, timeout, close mid-flight) has to be swallowed rather than propagated, while caller errors (malformed input, closed agent) throw synchronously before any async work starts. That split is the part most likely to be got wrong by reusing an existing request helper that rejects.

## Latent probe issue found nearby

Redirect policy is set on the `reqwest::Client` (`src/agent.rs:829`), so the raw client inherits it and defaults to following redirects. That means the existing H3 probe's `HEAD /` can be redirected to a different origin, and `H3Prober::spawn` then checks `response.version()` on the final response and confirms the **original** origin from another origin's transport.

Fixing that is its own card (see the [breakdown](../../breakdowns/d1/breakdown.md)). What matters here is that the warm-up must not inherit the same shape: `preconnect` does not follow redirects.
