# Follow-ups from the preconnect spec review

Cards spun out of D1 rather than a decomposition of it. D1 does not depend on any of them landing.

## Stop the HTTP/3 probe confirming an origin from a redirected response

The redirect policy is configured on the `reqwest::Client`, so the raw client the probe runs on inherits it and follows redirects by default. A probe's `HEAD /` can therefore be answered by a different origin than the one being probed, and the version check on the final response then confirms HTTP/3 for the original origin on the strength of another origin's transport. A confirmed origin routes foreground requests over QUIC, so a wrong confirmation sends real traffic down a path that was never verified.

The probe should judge only the origin it set out to probe. Whether that means not following redirects at all, or treating a redirected response as neither a confirmation nor a failure, is the design question. The eager HTTP/3 probing spec is also silent on redirects today, so it needs a criterion either way.
