# A3 — Bench the stale-DNS win in the features suite

Add a Faith-vs-Faith pair to the features suite's DNS group that shows what
serving stale DNS answers buys, now that both the feature (J1) and a harness
that can slow a DNS answer (W2) exist.

## The measurement

- One non-exempt name (`bench.test`) resolved through the harness's own
  controlled nameserver, reused from `test/lib/`, the same one the existing DNS
  rows use.
- The nameserver answers slowly (`SLOW_DNS_MS`), so a lookup is a visible part
  of what a request pays. Against an instant resolver the pair would both be
  fast and the row could not fail to impress.
- Two rows, identical but for `dns.serveStale`:
  - `dns:stale` — `serveStale: true`, serves the expired answer immediately and
    refreshes behind it, so the request never pays the resolver.
  - `dns:no-stale` — `serveStale: false`, discards the expired answer and blocks
    on a fresh (slow) lookup on every request.

## What the pair holds constant

Everything but `dns.serveStale`: same name, same nameserver, same answer delay,
same warm agent, same `/close` route, same TTL, same payload/concurrency grid.
The distance between the two rows is therefore the whole DNS cost that serving
stale removes from the request path, and nothing else.

## Arranging for entries to be expired (not cold, not fresh)

The three existing DNS rows run `cold` (a fresh agent per request, empty cache,
a full lookup every time). Serving stale needs the opposite: a cache that
persists so there is something to go stale.

- **Warm** mode, so the shared agent's DNS cache survives across requests.
- **TTL 0** from the nameserver, so every answer is expired the instant it
  lands. The warmup request populates the cache (so the measured requests are
  not cold); nothing ever stays fresh (so they are not fresh either); every
  measured request lands on an expired entry.
- **`/close`** route, so each request opens a new connection and actually
  consults the resolver. A reused keep-alive connection skips DNS entirely and
  would hide the difference.

Verified empirically: at TTL 0 against a 50ms nameserver, `serveStale: true`
answers in ~0.5ms (one background refresh) while `serveStale: false` pays ~50ms
on every request. At TTL 1 both sit at cache-hit speed — the win amortises away,
confirming TTL 0 is what puts every measured request on the expired path.

## What would count as not showing the win

The `dns:no-stale` row is the control. The claim is the distance between the two
rows, and it can fail:

- If `dns:stale` rose toward `dns:no-stale`, serving stale would not be keeping
  DNS off the request path.
- If `dns:no-stale` were as fast as `dns:stale`, the DNS cost never reached the
  measured path (a reused connection, a fresh entry, an exempt name) and neither
  row is evidence of anything.

The win shows only when `dns:no-stale` pays the resolver and `dns:stale` does
not. A `dns:stale` row read on its own is not evidence: it is fast whether or
not serving stale did any work.

## Checklist

- [x] Let a DNS variant request a zone TTL so `bench.test` can be served at TTL 0
- [x] Add `dns:stale` and `dns:no-stale` variants to the features DNS group
- [x] Document the pair and its falsification criterion in `bench/README.md`
- [x] Run the features DNS slice and confirm the gap (stale fast, no-stale slow)
- [x] Capture the scenarios in the card's test cases

Run at TTL 0 against a 50ms nameserver: `dns:stale` 0.30ms p50 (one background
refresh query), `dns:no-stale` 51.0ms p50 (a blocking lookup every request). The
gap is the resolver's full delay, removed from the path.
