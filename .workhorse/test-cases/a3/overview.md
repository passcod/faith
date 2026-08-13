# A3 — Bench the stale-DNS win

Scenarios for the `dns:stale` / `dns:no-stale` pair in the features suite. The
bench harness is not run by CI, so these are verified by running the suite and
reading the numbers; the underlying serve-stale behaviour is covered by
`test/agent-dns-stale.test.js`.

Run with:

```bash
node bench/run.mjs --suite features --sizes 1024 --conc 1 --samples 40 --warmup 20
```

## The pair shows the win

- [x] `dns:stale` and `dns:no-stale` both appear in the features output and in
      the DNS panel of `features-latency` / `features-rps`.
- [x] `dns:no-stale` pays the nameserver's answer delay on every request (total
      p50 near `SLOW_DNS_MS`), the same slow-resolver cost `dns:slow` shows.
- [x] `dns:stale` answers at cache-hit speed (total p50 well under the delay),
      so serving stale keeps the resolver off the request path. (verifies
      spec: DNS — serving stale answers)
- [x] The distance between the two rows is roughly the whole answer delay.

## Only the DNS knob moves

- [x] The pair differs only by `dns.serveStale`; name, nameserver, delay, warm
      mode, `/close` route, TTL, payload, and concurrency are shared.
- [x] `bench.test` is resolved through the harness's controlled nameserver (not
      the system resolver), so the delay knob actually reaches the lookup.

## Entries are expired, not cold or fresh

- [x] At TTL 0 every measured request lands on an expired entry: `dns:stale`
      issues a single background refresh query for the whole measured phase,
      while `dns:no-stale` issues one blocking query per request.
- [x] At a fresh TTL (e.g. 1s) the win disappears — both rows sit at cache-hit
      speed — confirming TTL 0 is what puts every request on the expired path.

## Falsification (the row must be able to fail)

- [ ] If serve-stale regressed so `dns:stale` paid the resolver, its row would
      rise to meet `dns:no-stale` and the pair would show no gap. (manual: the
      win is not self-proving; `dns:stale` read alone is fast regardless)
- [ ] If the DNS cost stopped reaching the path (reused connection, fresh entry,
      exempt name), `dns:no-stale` would drop to cache-hit speed and neither row
      would be evidence.
