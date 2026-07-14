# fáith benchmarks

This suite replaces the earlier exploratory `motivation/` benchmarks, whose
timing numbers included podman container setup, packet capture, tshark
filtering, and Node process startup in every sample — swamping the requests
being measured. Those numbers could not answer whether fáith itself was fast
or slow.

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
- **Warmup + distributions.** The first `--warmup` requests per scenario are
  discarded. p50/p90/p99/mean/stddev are reported; never a single total.
- **Phase split.** Time-to-headers (ttfb) and body drain are recorded
  separately per request, so protocol/connection effects and body-transfer
  effects don't get conflated.
- **Identical work.** All implementations consume the body the same way
  (`bytes()` by default). `--consume discard` exercises fáith's
  drop-without-copy path and is fáith-only, reported as its own scenario.
- **Cold vs warm.** Warm shares one agent/pool per scenario. Cold forces a
  fresh connection per request: the server sends `Connection: close` (h1),
  and each request gets a fresh agent (h2, fáith), so cold h2 numbers include
  agent construction — read them as "first request on a new client".
- **Event-loop health.** `monitorEventLoopDelay` runs during each scenario;
  p99 loop delay shows how much an implementation blocks JavaScript while
  moving bytes. This is a first-class result, not a curiosity: it's the
  metric where offloading I/O to another thread should show up.
- **Closed loop.** `--conc N` runs N workers issuing requests back to back.
  Throughput (rps) is derived from wall time of the measured window.

Packet-level behaviour (connection counts, DNS queries, negotiated protocol)
is deliberately **not** measured here; when needed, capture it in a separate
untimed pass so it can't perturb latency numbers.

## Running

```bash
npm run build          # release build of the addon first
node bench/run.mjs                     # quick suite: h1+h2, 1k/64k, c1/c16
node bench/run.mjs --suite full        # full matrix incl. h3, cold+warm
node bench/run.mjs --suite features    # fáith vs fáith across feature knobs
node bench/run.mjs --protos h1 --sizes 65536 --conc 64 --samples 1000
node bench/run.mjs --protos h3         # HTTP/3 (fáith only; see below)
node bench/run.mjs --consume discard --protos h2   # fáith-only discard path
node bench/run.mjs --delay 20          # +20ms server-side response delay
```

### HTTP/3

Node has no HTTP/3 server, so h3 scenarios are served by
`examples/h3-server.rs` (quinn + h3, the same stack fáith consumes through
reqwest), which the harness builds with cargo on first use and spawns per
scenario. It serves the same routes with byte-identical payloads. On
IPv4-only hosts the fáith agent is given `localAddress: "0.0.0.0"`, since the
QUIC endpoint otherwise binds the IPv6 wildcard.

### Feature comparisons (`--suite features`)

fáith against itself, one knob per row, grouped: HTTP versions (h1/h1s/h2/h3,
same workload), DNS (hickory + cache vs system resolver, in cold mode so
resolution is on the measured path), IPv4 vs IPv6 loopback, HTTP cache (none
vs memory vs disk store, on a cacheable route where warm requests are all
hits), and cookie jar off/on. Cross-implementation comparisons deliberately
don't apply here — these rows answer "what does enabling this feature cost
(or save) me", not "who is faster".

Raw per-scenario records (with full summaries) are appended as NDJSON under
`bench/results/`. `node-fetch` scenarios run only if it is installed
(`npm i -D node-fetch`).

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

Loss × HTTP/3 is the headline scenario HTTP/3 exists for; the local suite
can't cover h3 (Node has no h3 server), so point `--protos h3` runs at an
external server you control (Caddy with `experimental_http3`, nginx-quic)
once an `--h3-url` mode is added.

## Interpreting

- fáith pays a NAPI boundary cost per request and per body: expect a fixed
  tax on small warm h1 requests versus undici. The question is its size.
- Wins should be looked for in: h2 at `--conc 16+`, `--consume discard`,
  event-loop p99 under large `--sizes`, and (externally) h3 under loss.
- Cold-mode h2 fáith numbers include full agent construction; compare cold
  fáith against cold fáith across changes, not against native cold.
