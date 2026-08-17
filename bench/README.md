# Faith benchmarks

## Implementations

Nearly every JS HTTP client sits on one of three transport stacks, so benching
two wrappers over the same stack mostly measures wrapper overhead. The set is
picked to have at most one representative per (stack, API-style):

| impl | stack | protocols | notes |
|------|-------|-----------|-------|
| `native` | undici (standard fetch) | h1, h1s | Node's built-in `fetch()` |
| `undici` | undici (raw) | h1, h1s | `undici.request()` (same underlying API, but with less wrapping) |
| `http2` | node core | h2 | `node:http2` client (native, built-in) |
| `got` | node core / http2-wrapper | h1, h1s, h2 | a popular JS client that actually speaks h2 |
| `node-fetch` | node core | h1, h1s | most common non-built-in `fetch()` |
| `libcurl` | curl (C) | h1, h1s, h2 | also uses NAPI, so most comparable overhead |
| `faith` | reqwest (Rust) | h1, h1s, h2, h3 | this project |

h1s is HTTP/1 + TLS, h2 and h3 both include TLS directly. We don't support h2c.

Popular clients deliberately excluded: axios, ky, ofetch, superagent, needle...
are all wrappers over the undici or node-core stacks already represented, with
nothing new at the transport level.

Notes:
- **libcurl** runs with peer verification disabled. Its `node-libcurl` prebuilt
  statically links its own OpenSSL with a baked CA path and ignores `CAINFO`,
  `CURL_CA_BUNDLE`, and `SSL_CERT_FILE`, so the private-CA bench cert can't be
  trusted portably. The full TLS handshake and record crypto still run; but the
  whole certificate chain check is skipped, which does save some time.
- **libcurl** buffers the whole body before resolving, so we can't accurately
  measure `ttfb` (we record it the same as `total`). Every other client resolves
  at response headers, so `ttfb` is really time-to-end-of-headers.
- libcurl and the node core / undici stacks only load the TLS trust store once per
  process, while **Faith** does this on every `new Agent()` construction. This
  completely kills our "cold" benchmark numbers by an order of magnitude or more.
  This is not the usual pattern, though.

## Running

```bash
npm run build
npm install --prefix bench

node bench/run.mjs                      # quick suite: h1+h2, 1k/64k, c1/c16
node bench/run.mjs --suite full         # full matrix incl. h3, cold+warm
node bench/run.mjs --suite concurrency  # concurrency sweep: c1…c128, warm, for the throughput curve
node bench/run.mjs --suite features     # Faith vs Faith across feature knobs

# advanced / custom scenarios
node bench/run.mjs --delay 20           # +20ms server-side response delay
node bench/run.mjs --protos h1 --sizes 65536 --conc 64 --samples 1000
```

### Charts

`bench/plot.mjs` turns a results file into a handful of gnuplot charts.
It needs gnuplot on `PATH` (`apt install gnuplot-nox`, `brew install gnuplot`).

```bash
node bench/plot.mjs                       # newest results file → SVGs in results/plots/
node bench/plot.mjs --format png          # PNG instead of SVG
node bench/plot.mjs --in results/bench-….ndjson --out /tmp/plots
node bench/plot.mjs --size 65536 --conc 64 --mode warm   # pick which slice to chart
```

It selects and renders only a single slice (largest concurrency, mid payload size, warm);
this can be overriden with `--size`/`--conc`/`--mode`; the chosen slice is printed.

Charts:

- `latency-by-impl`: grouped total p50/p99 bars per implementation
- `throughput`: requests/s vs concurrency, a line per implementation
- `latency-vs-size`: total p50 vs payload size, a line per implementation
- `loop-delay`: event-loop-delay p99 per implementation

When a "features" bench is run, then these charts are rendered instead:

- `features-latency`: one panel per feature group (versions, DNS, address
family, cache, cookies) of total p50 latency.
- `features-rps`: one panel per feature group (versions, DNS, address
family, cache, cookies) of throughput.

### The DNS group

The DNS rows resolve `bench.test`, a name that is not exempt, through a
nameserver the harness controls (the authoritative server under `test/lib/`,
reused rather than reimplemented). `localhost` would be handed to the system
resolver whatever the agent's DNS settings say, so both rows would resolve the
same way and measure nothing. The `hickory`, `slow`, and `system` rows run cold,
so a lookup is on the measured path of every request rather than amortised by
the cache.

- `dns:hickory`: Faith's own resolver, against the controlled nameserver
  answering instantly.
- `dns:slow`: the same, but the nameserver answers slowly. Identical to
  `dns:hickory` in every other respect, so the distance between them is the DNS
  cost and nothing else.
- `dns:system`: the OS resolver (`dns.system`), resolving `localhost`. A
  reference for handing off to the system, which cannot be pointed at the
  controlled nameserver.

#### Serving stale answers

`dns:stale` and `dns:no-stale` are a Faith-vs-Faith pair over the same slow
nameserver, identical but for `dns.serveStale`. Serving stale answers an expired
lookup from cache immediately and refreshes behind it, so the request never pays
the resolver; turning it off discards the expired answer and blocks on a fresh
(slow) lookup. The distance between the two rows is the whole DNS cost that
serving stale removes from the request path.

The pair holds everything else constant. Both need a cache that persists, which
the cold rows (a fresh agent per request) have nothing in, so both run warm.
Both answer at a TTL of 0, so every answer is expired the instant it lands: the
warmup populates the cache, so the measured requests are not cold, and nothing
ever stays fresh, so they are not fresh either — every measured request lands on
an expired entry. Both fetch the `/close` route, so each request opens a new
connection and actually consults the resolver rather than reusing a connection
that would skip DNS entirely. Only `dns.serveStale` differs.

`dns:no-stale` is the control, and the win is the distance between the two rows,
which can fail to show:

- `dns:stale` rising toward `dns:no-stale` means serving stale is no longer
  keeping DNS off the request path.
- `dns:no-stale` being as fast as `dns:stale` means the DNS cost never reached
  the measured path — a reused connection, a fresh entry, an exempt name — and
  neither row is evidence of anything.

Read on its own `dns:stale` is not evidence: it is fast whether or not serving
stale did any work. The win is real only when `dns:no-stale` pays the resolver
and `dns:stale` does not.

## Simulating latency and loss

Experiemental, not well-measured / optimised for yet. This is what h3 is
nominally good at but no other Node.js client supports it at this time.

```bash
sudo ip netns add bench
sudo ip link add veth0 type veth peer name veth1 netns bench
# ... assign addrs, then:
sudo ip netns exec bench tc qdisc add dev veth1 root netem delay 20ms loss 1%
```

