# fáith benchmarks

An in-process HTTP benchmark harness for fáith and comparable clients, built
so that request timing is what's actually measured.

## Questions this suite answers

1. **What is the overhead tax?** How much slower is fáith than native
   `fetch()` (undici) in undici's best case: warm HTTP/1.1 keep-alive
   requests to a local server. If this is small, the feature set (h2/h3,
   caching, pooling controls) carries the project on its own.
2. **Where does fáith win?** Scenarios undici structurally can't play in:
   HTTP/2 multiplexing under concurrency, HTTP/3 (external server required),
   and keeping heavy transfer work off the JS event loop.
3. **Did we regress?** The quick suite is stable enough to run before/after
   a change.

## Methodology

- **In-process timing.** One long-lived Node process; module load and first
  agent construction are excluded from samples. Startup cost is a separate
  metric, not smeared into request latency.
- **Local servers, controlled payloads.** `bench/lib/servers.mjs` serves
  deterministic bodies of exact sizes over h1, h1 + TLS, and h2 (TLS,
  `allowHTTP1: false`). The network is not the variable; if you want RTT and
  loss as variables, use `tc netem` (below).
- **Warmup is a separate, untimed phase.** The first `--warmup` requests per
  scenario run before the clock starts — they aren't recorded *and* they don't
  count toward the wall time rps is derived from. (Folding warmup into the
  timed loop, as an earlier version did, silently deflated every rps.)
  p50/p90/p99/mean/stddev are reported; never a single total.
- **Phase split.** Time-to-headers (ttfb) and body drain are recorded
  separately per request, so protocol/connection effects and body-transfer
  effects don't get conflated.
- **Identical work.** All implementations consume the body the same way
  (`bytes()` by default). `--consume discard` exercises fáith's
  drop-without-copy path and is fáith-only, reported as its own scenario.
- **Cold vs warm.** Warm shares one client for the whole scenario. Cold means
  "first request on a fresh client": a fresh client is built *per request*, and
  because building it is part of what cold measures, that construction is
  **inside** the timed window (teardown is not) — so cold latency and cold rps
  agree, both including setup. Note the clients aren't symmetric here: `native`
  reuses the global undici agent and only forces `Connection: close`; `undici`
  and `faith` build and then free a client each request (`faith` via the
  `Agent.close()` added for exactly this — otherwise fresh agents pile up until
  GC and poison the run). Read cold as an intra-client warm-vs-cold delta, not
  a cross-client race — and expect it to argue loudly for reusing one agent.
- **Event-loop health.** `monitorEventLoopDelay` runs during each scenario;
  p99 loop delay shows how much an implementation blocks JavaScript while
  moving bytes. This is a first-class result, not a curiosity: it's the
  metric where offloading I/O to another thread should show up.
- **Closed loop.** `--conc N` runs N workers issuing requests back to back.
  Throughput (rps) = sampled requests ÷ wall time of the (warmup-excluded)
  measured window.

Packet-level behaviour (connection counts, DNS queries, negotiated protocol)
is deliberately **not** measured here; when needed, capture it in a separate
untimed pass so it can't perturb latency numbers.

## Implementations

Nearly every JS HTTP client sits on one of three transport stacks, so benching
two wrappers over the same stack mostly measures wrapper overhead. The set is
picked to have at most one representative per (stack, API-style):

| impl | stack | protocols | notes |
|------|-------|-----------|-------|
| `native` | undici (spec fetch) | h1, h1s | Node's built-in `fetch()` |
| `undici` | undici (raw) | h1, h1s | `undici.request()` — the stack's ceiling, no WHATWG-stream tax |
| `http2` | node core | h2 | `node:http2` client — the only builtin h2, raw-core baseline |
| `got` | node core / http2-wrapper | h1, h1s, h2 | the popular JS client that actually speaks h2 |
| `node-fetch` | node core | h1, h1s | spec-shaped wrapper over core |
| `libcurl` | native (libcurl) | h1, h1s, h2 | via `node-libcurl`; fáith's native-bindings cousin |
| `faith` | native (reqwest/hyper) | h1, h1s, h2, h3 | this project |

Clients popular but not included — axios, ky, ofetch, superagent, needle —
are all wrappers over the undici or node-core stacks already represented, with
nothing new at the transport level.

Two caveats:
- **libcurl** runs with peer verification disabled. Its `node-libcurl` prebuilt
  statically links its own OpenSSL with a baked CA path and ignores `CAINFO`,
  `CURL_CA_BUNDLE`, and `SSL_CERT_FILE`, so the private-CA bench cert can't be
  trusted portably. The full TLS handshake and record crypto still run; only
  the (per-connection, keep-alive-amortized) chain check is skipped.
- **libcurl** (via `curly`) buffers the whole body before resolving, so its
  ttfb equals total. Every other client resolves at response headers, giving a
  true time-to-headers.

## Running

The benchmark harness and its comparison clients are a **separate package**
(`bench/package.json`) so they never touch fáith's own dependencies or
published artifact. Build the addon once at the repo root, install the bench
deps once inside `bench/`, then run from the root:

```bash
npm run build              # release build of the addon (repo root)
npm install --prefix bench # comparison clients (undici, got, node-fetch, node-libcurl)

node bench/run.mjs                     # quick suite: h1+h2, 1k/64k, c1/c16
node bench/run.mjs --suite full        # full matrix incl. h3, cold+warm
node bench/run.mjs --suite concurrency  # concurrency sweep: c1…c128, warm, for the throughput curve
node bench/run.mjs --suite features    # fáith vs fáith across feature knobs
node bench/run.mjs --protos h1 --sizes 65536 --conc 64 --samples 1000
node bench/run.mjs --protos h3         # HTTP/3 (fáith only; see below)
node bench/run.mjs --consume discard --protos h2   # fáith-only discard path
node bench/run.mjs --delay 20          # +20ms server-side response delay
```

### HTTP/3

Node has no HTTP/3 server, so h3 scenarios are served by the standalone
`bench/h3-server` crate (quinn + h3, the same stack fáith consumes through
reqwest), which the harness builds with cargo on first use and spawns per
scenario. It serves the same routes with byte-identical payloads. On
IPv4-only hosts the fáith agent binds `0.0.0.0` for QUIC automatically (the
QUIC endpoint otherwise binds the IPv6 wildcard and can't be constructed
there); no manual `localAddress` is needed.

### Feature comparisons (`--suite features`)

fáith against itself, one knob per row, grouped: HTTP versions (h1/h1s/h2/h3,
same workload), DNS (hickory + cache vs system resolver, in cold mode so
resolution is on the measured path), IPv4 vs IPv6 loopback, HTTP cache (none
vs memory vs disk store, on a cacheable route where warm requests are all
hits), and cookie jar off/on. Cross-implementation comparisons deliberately
don't apply here — these rows answer "what does enabling this feature cost
(or save) me", not "who is faster".

Raw per-scenario records (with full summaries) are appended as NDJSON under
`bench/results/`. The pure-JS comparison clients live in `bench/package.json`
and are pulled in by `npm install --prefix bench`. node-libcurl is a native
addon and an `optionalDependency` there: it builds and participates on most
platforms, but if its addon can't build the harness skips only the libcurl
rows (with a note) rather than failing.

TLS scenarios use a generated self-signed certificate; the runner re-execs
itself with `NODE_EXTRA_CA_CERTS` so native fetch trusts it, and passes the
same CA to fáith via the `tls.extraRoots` agent option.

## Simulating real networks

Run the server inside a network namespace with latency/loss applied, e.g.:

```bash
sudo ip netns add bench
sudo ip link add veth0 type veth peer name veth1 netns bench
# ... assign addrs, then:
sudo ip netns exec bench tc qdisc add dev veth1 root netem delay 20ms loss 1%
```

Loss × HTTP/3 is the headline scenario HTTP/3 exists for, and the local h3
server (`bench/h3-server`) runs inside the namespace like any other, so
`--protos h3` under `netem loss` is the experiment to reach for.

## Interpreting

- fáith pays a NAPI boundary cost per request and per body: expect a fixed
  tax on small warm h1 requests versus undici. The question is its size.
- Wins should be looked for in: h2 at `--conc 16+`, `--consume discard`,
  event-loop p99 under large `--sizes`, and (externally) h3 under loss.
- Cold-mode h2 fáith numbers include full agent construction; compare cold
  fáith against cold fáith across changes, not against native cold.
