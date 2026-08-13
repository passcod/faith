# Restore the DNS comparison in the features benchmark (W2)

The deliverable is a benchmark harness (`bench/`, not part of the published
package), so these are manual verification scenarios run with `node
bench/run.mjs --suite features`, not automated unit tests.

## DNS group measures distinct paths

- [x] `dns:hickory`, `dns:slow`, and `dns:system` each report different total
      latency in a features run — the old two rows were identical because both
      resolved `localhost` through the system resolver
- [x] `dns:slow` sits above `dns:hickory` by about the nameserver answer delay
      (~50ms), confirming the added latency is the DNS answer and nothing else
- [x] `dns:hickory` resolves `bench.test` (a non-exempt name) through the
      controlled nameserver rather than being handed to the system resolver — the
      ~50ms `dns:slow` delta can only appear if the query reaches that nameserver

## Harness plumbing

- [x] The controlled nameserver is started per DNS variant, pointed at the HTTP
      server's own loopback address, and torn down after the variant
- [ ] A run leaves no nameserver sockets bound after it exits (clean teardown of
      both UDP and TCP on the ephemeral port)
- [x] The non-DNS feature groups (versions, address family, cache, cookies) are
      unchanged by the DNS wiring
