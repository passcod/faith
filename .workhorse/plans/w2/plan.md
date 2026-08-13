# Restore the DNS comparison in the features benchmark (W2)

Spun off from J1. Bench-only change under `bench/`; no product behaviour changes,
so no spec edits. Stale serving (J1) is specced but not yet implemented, and this
card does not depend on it: the rows are wrong on their own terms and the fix
stands regardless.

## The bug

`runFeatures()` in `bench/run.mjs` has a DNS group with two rows, `dns:hickory`
and `dns:system`, both requesting `localhost`. H1 (#78) made `localhost` an
always-exempt name handed to the system resolver whatever `dns.servers` says, so
both rows resolve through the same system resolver and the comparison measures
nothing. `bench/` was untouched by H1, so this is live on main.

Separately, the harness can delay an HTTP response (`--delay`) but not a DNS
answer, so no row can show a slow resolver — which is the only condition under
which Faith's resolver and its cache earn their keep.

## Decisions this card carries

- **Reuse the nameserver under `test/lib/dns-server.js`, don't write a second
  one.** It is a controllable authoritative nameserver (UDP + TCP, ephemeral
  port) with a settable zone, per-name TTL, a settable answer delay, and a query
  log, and its wire format is already validated by `test/dns-server.test.js`
  against Node's c-ares resolver. Its `delayMs` knob is exactly the DNS-delay
  capability the harness was missing, so wiring it in closes that gap.

- **The system resolver cannot be pointed at the controlled nameserver.**
  `dns.system: true` resolves via `getaddrinfo`, which reads the OS config; a
  bench cannot rewrite `/etc/resolv.conf`. So the held-constant, DNS-cost-only
  comparison is hickory-vs-hickory (same resolver, same name, same nameserver,
  same cold connection setup — only the answer delay differs). `dns:system`
  stays as a reference point for the OS path, resolving `localhost`.

- **What the slow-resolver row holds constant.** `dns:hickory` and `dns:slow`
  are identical cold rows — Faith's own resolver, the same non-exempt name
  (`bench.test`) resolved through the same controlled nameserver to the same
  loopback HTTP server, the same payloads and concurrency — differing only in
  the nameserver's answer delay. The delta between them is pure DNS cost. Search
  domains and ndots are pinned (`searchDomains: []`, `ndots: 0`) so a name
  becomes a query the same way on every machine.

## DNS group rows

- `dns:hickory` — Faith's resolver via the controlled nameserver, answer delay
  0, `bench.test`, cold. Baseline: a local resolver.
- `dns:slow` — as `dns:hickory` but the nameserver answers slowly. The
  slow-resolver row; its distance from `dns:hickory` is the DNS cost.
- `dns:system` — the OS resolver (`dns.system: true`), `localhost`, cold.
  Reference for handing off to the system.

## Steps

- [x] Re-export `startDnsServer` from `bench/lib/servers.mjs`
- [x] Wire a controlled nameserver per DNS variant in `runFeatures()`: start it
      with a `bench.test` zone pointing at the HTTP server's loopback address and
      the variant's answer delay, inject `dns.servers`/`timeout`/`searchDomains`/
      `ndots` into the variant's agent options, tear it down after the variant
- [x] Replace the two `localhost` DNS rows with `dns:hickory`, `dns:slow`,
      `dns:system`
- [x] Update `bench/README.md` to describe the restored DNS group
- [x] Run `--suite features` and confirm the three DNS rows measure distinct
      things (slow row clearly above the other two): `dns:hickory` 14.9ms /
      `dns:slow` 65.4ms / `dns:system` 5.5ms — the ~50ms gap is exactly the
      injected answer delay, so the DNS cost is the only thing moving
