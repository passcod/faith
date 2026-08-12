# Follow-ups from the preconnect spec review

Cards spun out of D1 rather than a decomposition of it. D1 does not depend on any of them landing.

## Stop the HTTP/3 probe confirming an origin from a redirected response

The redirect policy is configured on the `reqwest::Client`, so the raw client the probe runs on inherits it and follows redirects by default. A probe's `HEAD /` can therefore be answered by a different origin than the one being probed, and the version check on the final response then confirms HTTP/3 for the original origin on the strength of another origin's transport. A confirmed origin routes foreground requests over QUIC, so a wrong confirmation sends real traffic down a path that was never verified.

The probe should judge only the origin it set out to probe. Whether that means not following redirects at all, or treating a redirected response as neither a confirmation nor a failure, is the design question. The eager HTTP/3 probing spec is also silent on redirects today, so it needs a criterion either way.

## Take redirect-following into Fáith's own layer so synthetic traffic never follows

`reqwest`'s redirect policy is per-client and immutable after build, and its connection pool is per-client too, so the raw client the probe and `preconnect` share to pool into the foreground cannot follow one policy while foreground requests follow another. Today that forces `preconnect` to inherit the agent's redirect policy (following by default) rather than staying on the origin it was asked to warm. Moving redirect-following into a middleware in Fáith's stack lets the underlying `reqwest::Client` be `Policy::none()`, so all synthetic background traffic — the HTTP/3 probe and the `preconnect` warm-up alike — stops at the first response, while foreground requests keep following through the middleware. This subsumes the probe-redirect fix above and lets `preconnect` return to not following redirects. It re-implements reqwest's redirect behaviour (the follow limit, the `error`/`stop`/`manual` modes, cross-origin sensitive-header stripping, and `Referer`), so it is sized as its own card.
