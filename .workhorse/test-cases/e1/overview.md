# Network-change signal test cases

Scenarios verifying `agent.networkChanged()` against [NETCHG](../../specs/agent/network-change.md).

State-level behaviour of the HTTP/3 origin knowledge is covered by Rust unit tests in
`src/alt_svc.rs`, since the states are not observable from JS; everything a caller can see is
covered from JS in `test/agent-network-change.test.js`.

## What the signal resets

- [x] The pooled connection is closed by the signal, not merely bypassed (verifies spec: NETCHG)
- [x] A request after the signal opens a new connection instead of reusing the pooled one (verifies spec: NETCHG)
- [x] A `preconnect`ed origin is no longer warm after the signal, so warming it again does real work (verifies spec: NETCHG)
- [x] A request after the signal resolves names again and succeeds (verifies spec: NETCHG)
- [x] Under `dns.system` the signal does no DNS work and requests keep working (verifies spec: NETCHG)
- [x] An observation-confirmed HTTP/3 origin demotes to advertised and becomes probe-worthy (verifies spec: NETCHG)
- [x] The HTTP/3 failed state and its cooldown clear, and the next failure starts from the base cooldown (verifies spec: NETCHG)
- [x] A slow-demoted HTTP/3 origin re-enters through a probe immediately (verifies spec: NETCHG)
- [x] The path-time averages clear, so one family's samples alone cannot demote (verifies spec: NETCHG)
- [x] Probe single-flight claims are released, so a fresh probe can start at once (verifies spec: NETCHG)
- [x] A confirmation that had already lapsed does not come back as a fresh advertisement (verifies spec: NETCHG)

## What the signal keeps

- [x] `http3.hints` keep their origins confirmed and unprobed (verifies spec: NETCHG)
- [x] A hint that a failure cooldown was masking takes effect once the failures clear (verifies spec: NETCHG)
- [x] An `Alt-Svc` advertisement survives the signal (verifies spec: NETCHG)
- [x] The cookie jar keeps its cookies (verifies spec: NETCHG)
- [x] An in-memory HTTP cache still serves a hit after the signal rather than refetching (verifies spec: NETCHG)
- [x] The `stats()` counters are unchanged by the signal (verifies spec: NETCHG)
- [x] Default headers and `userAgent` still apply to requests after the client is rebuilt (verifies spec: NETCHG)
- [x] A disk HTTP cache still serves a hit after the signal (verifies spec: NETCHG)
- [x] `tls.extraRoots` trust still applies after the signal (verifies spec: NETCHG)
- [x] The configured flow-control window still reaches the wire after the signal (verifies spec: NETCHG)
- [x] `localAddress` still binds the same source IP after the signal (verifies spec: NETCHG)
- [x] Redirect and timeout options still apply after the signal (verifies spec: NETCHG)
- [x] `connections()` keeps listing a connection carrying an in-flight request across the signal (verifies spec: NETCHG)
- [ ] `tls.identity` (client certificate) still applies after the signal (verifies spec: NETCHG)
- [ ] Pool options (`idleTimeout`, `maxIdlePerHost`) still apply to the rebuilt pool (verifies spec: NETCHG)

## In-flight requests

- [x] A request awaiting a response completes normally when the signal lands mid-flight (verifies spec: NETCHG)
- [x] A response body still downloading is not interrupted by the signal (verifies spec: NETCHG)
- [x] A `preconnect` in flight across the signal does not leave its origin marked warm (verifies spec: NETCHG)

## Availability

- [x] Repeated signals on an agent with nothing to reset are harmless (verifies spec: NETCHG)
- [x] The signal on a closed agent does nothing and does not throw, and the agent stays closed (verifies spec: NETCHG)

## Regressions locked in

- [x] `dns.overrides` resolve a name the system resolver cannot, with `dns.system: true` (verifies spec: DNS)
- [x] An unparseable `dns.overrides` address throws at construction with `dns.system: true` (verifies spec: AGENT)
