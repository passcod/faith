# Import all FR issues

Each open `FR:` issue on `passcod/faith` becomes one card. Entries are ordered by cluster and dependency rather than issue number: the HTTP/3 and Alt-Svc work first, then connection resilience and caching, then API surface, then the larger open-ended pieces. Closed FR issues (FormData, `Response.redirected`, `FetchOptions.integrity`, `URLSearchParams` bodies) are already shipped and are not imported, and non-FR issues are left alone.

## Back off the HTTP/3 broken cooldown exponentially · B1

A failed HTTP/3 origin is parked in the Alt-Svc `failed` cache for a fixed `upgradeFailedTtl`, so an origin whose UDP path is permanently blocked is retried every five minutes forever. Keep a consecutive-failure count per origin, derive the cooldown from it by doubling up to a cap, and clear the count on a confirmed HTTP/3 success. Scope is the failed cache and its TTL computation; the eager-probe machinery itself is untouched. Tracked as passcod/faith#47.

## Read HTTPS/SVCB DNS records as an HTTP/3 hint · C1

hickory can query the `HTTPS` record type (RFC 9460) alongside A/AAAA, so an origin advertising `alpn="h3"` can be learnt before the first connection instead of waiting for an `Alt-Svc` header to arrive over TCP. Feed such a record into the Alt-Svc cache as hint-grade evidence for the background probe to verify, which removes Alt-Svc's need for a TCP round trip to discover HTTP/3 at all. ECH configs carried by the same record are out of scope until rustls ECH support settles. Tracked as passcod/faith#46.

## Add preconnect() and prefetchDns() agent APIs · D1

Expose the browser's `dns-prefetch` and `preconnect` verbs on the agent: `agent.prefetchDns(host)` warms the DNS cache, and `agent.preconnect(origin)` resolves and opens a connection ahead of the first request so it skips setup round trips. The eager-probe machinery covers most of the HTTP/3 half; the TCP side needs an equivalent HTTP/1 and HTTP/2 warm-up so `preconnect()` means the same thing whichever protocol the origin lands on. Design has to pin down what the call promises, whether it settles on establishment, and how it interacts with pool idle timeouts. Tracked as passcod/faith#48.

## Add a network-change signal API · E1

Node has no portable signal for interface or network changes, so expose the reaction instead and let the caller wire up their own trigger: `agent.networkChanged()` drops pooled connections, flushes the DNS cache, clears the Alt-Svc `failed` and `slow` states along with any cooldown backoff, and resets path-time EWMAs. Most of these resets exist per subsystem already, so the work is plumbing them into one verb and documenting its limits, in particular that in-flight requests are not interrupted. Tracked as passcod/faith#56.

## Retry transparently when a reused pooled connection dies · F1

A reused connection that dies before any response bytes arrive can be retried once on a fresh connection without risk, even for non-idempotent requests, because nothing was processed. Start by adding a conformance-matrix dimension for origins that close idle connections aggressively, to establish whether hyper and reqwest already cover this or whether callers see spurious `ECONNRESET`s. If there is a gap, retry the cloneable cases at the middleware layer the way the HTTP/3 clone-fallback does; streaming bodies stay unretryable. Tracked as passcod/faith#51.

## Serve stale cache entries per RFC 5861 · G1

Support `stale-while-revalidate` (serve the stale entry immediately, revalidate in the background) and `stale-if-error` (fall back to the stale entry when revalidation fails with a network or 5xx error). First step is discovery of what `http-cache` and `http-cache-semantics` already support, preferring an options flip, then an upstream contribution, then middleware-layer behaviour. Background revalidation needs a task handle and single-flight per cache key. Tracked as passcod/faith#50.

## Support configurable alternative DNS transports · H1

Expose the resolver transports hickory supports, DoH and DoT among them, at the agent configuration level, with system discovery where that is appropriate. Scope is configuration and resolver wiring rather than cache policy. Tracked as passcod/faith#57.

## Serve stale DNS entries while revalidating · J1

Return expired DNS cache entries immediately and refresh them in the background, on the basis that IPs rarely change and a wrong IP fails fast with the retry and fallback machinery as the safety net. Determine whether hickory's cache policy can be configured for this or whether a thin wrapper cache is needed. The win only shows against slow resolvers, so this needs a benchmark row in the existing harness before it ships. Tracked as passcod/faith#54.

## Expose a per-request timing breakdown · K1

Attach a Resource Timing style breakdown to the response covering request start, request sent, first byte, body end, connection reuse, and negotiated protocol. reqwest exposes total time, reuse, and remote address readily; DNS, connect, and TLS phase boundaries are not reachable without upstream hooks, and the partial version carries most of the practical value. Name fields after `PerformanceResourceTiming` where they map, and share the instrumentation point with the per-protocol path-time EWMA so both read the same measurements. Tracked as passcod/faith#52.

## Surface 103 Early Hints · L1

Surface the `Link` headers of `103 Early Hints` interim responses so applications can preconnect or prefetch while the server is still working, as undici does via an event. Feasibility depends on whether hyper and reqwest expose 1xx handling before the middleware layer sees the response; if interim responses are swallowed, the outcome of this card is an upstream ask rather than a fáith feature. Pairs with the preconnect APIs, which give callers something to act on. Tracked as passcod/faith#53.

## Map the fetch priority option onto the RFC 9218 Priority header · M1

Implement the fetch spec's `priority: "high" | "low" | "auto"` request option by emitting an RFC 9218 `Priority` header, which is what modern HTTP/2 and HTTP/3 servers actually honour, rather than reaching for stream-priority knobs reqwest does not expose. Map `high` to a low urgency value, omit the header for `auto`, and use a high urgency value for `low`. An explicit `Priority` header set by the caller wins. Tracked as passcod/faith#49.

## Send request trailers · N1

Implement the fetch spec's `trailers: Promise<Headers>` on `RequestInit`. The body path has to carry frames rather than raw `Bytes` so a trailers frame survives into reqwest, non-streaming bodies need a one-shot equivalent, and HTTP/1.1 needs a `Trailer` header derived from the resolved header names or hyper drops the fields silently. HTTP/2 carries them natively; reqwest's HTTP/3 body pump discards them, so this card first has to decide whether a request carrying trailers is rejected on HTTP/3 or suppresses the upgrade, since that choice shapes the rest. Verification wants a conformance row that reports the trailers an origin received. Tracked as passcod/faith#44.

## Modernise the cookie jar for RFC 6265bis · P1

Bring the jar up to the RFC 6265bis rules that make sense server-side: enforce the `__Host-` and `__Secure-` prefix requirements, cap cookie count and size per origin so a misbehaving server cannot grow the jar without bound, and cap expiry at 400 days. `SameSite` stays out, being meaningless without a first-party browsing context. Scope depends on what reqwest's `Jar` and the `cookie` crate enforce already; fáith holds the `Arc<Jar>`, so swapping in a stricter store is contained. Tracked as passcod/faith#55.

## Support full duplex mode · Q1

Allow a request to stream its body while the response is already being read, rather than requiring the request body to complete first. Scope, protocol support, and API shape all need establishing, since the issue records the request without a design. Tracked as passcod/faith#3.

## Investigate BBRv3 congestion control · R1

Look into BBRv3 for the QUIC path, including the state of the stalled upstream effort, and decide whether fáith carries the work, waits, or contributes. This is an investigation card first; whether it produces an implementation depends on what upstream turns out to have. Tracked as passcod/faith#5.

## Expose a Rust API and publish to crates.io · S1

Fáith has grown well past being reqwest for Node, so the functionality is worth using directly from Rust. Design a sensible Rust-facing API over the existing internals, separate it cleanly from the napi binding layer, and publish to crates.io. Tracked as passcod/faith#58.
