# Preconnect and DNS prefetch

Working notes for D1. The behaviour is specified in [WARM](../../specs/agent/warm-up.md); this file holds the technical constraints the spec review turned up.

## The pool cannot be populated without a request

`hyper-util`'s pool insert (`Pool::put`) is private, its legacy pool module is not reachable from `reqwest`'s public API, and `reqwest` 0.13.4 exposes no preconnect or pool-warming call. So a TCP warm-up has to send a real request, which is why `preconnect` is specified as sending a synthetic `HEAD` to the origin root. `H3Prober` in `src/alt_svc.rs` already works this way and is the model to follow.

If a future `reqwest` gains a pool-warming API, the synthetic request becomes an implementation choice again rather than a constraint, and the spec's "what preconnect sends" section is what would need revisiting.

## Cache bypass and probe triggering pull in opposite directions

The H3 probe sends its `HEAD` on the **raw** `reqwest::Client` rather than the middleware stack, which is what gets it the HTTP cache bypass and stops probes recursing. A warm-up needs the same cache bypass, but the spec also has a TCP warm-up to a probe-worthy origin trigger a background HTTP/3 probe, and that trigger lives in `AltSvcMiddleware`. Copying the probe's raw-client approach wholesale would satisfy the cache requirement and silently drop the probe trigger.

So the warm-up needs one of: the middleware stack with the cache layer disabled for this request, or the raw client plus an explicit probe trigger on the way past. Worth deciding before writing the TCP path, because it is easy to get the cache half right and not notice the probe half is missing.

## Timeouts bound the warm-up, and it never rejects

The promise resolves and never rejects, so every failure path (DNS, refused, timeout, close mid-flight) has to be swallowed rather than propagated, while caller errors (malformed input, closed agent) throw synchronously before any async work starts. That split is the part most likely to be got wrong by reusing an existing request helper that rejects.

## Latent probe issue found nearby

Redirect policy is set on the `reqwest::Client` (`src/agent.rs:823`), so the raw client inherits it and defaults to following redirects. That means the existing H3 probe's `HEAD /` can be redirected to a different origin, and `H3Prober::spawn` then checks `response.version()` on the final response and confirms the **original** origin from another origin's transport. Not this card's behaviour to fix, but the warm-up must not inherit the same shape: `preconnect` does not follow redirects.
