# Multi-server conformance test matrix

Date: 2026-08-05
Issue: #25

## Problem

faith's tests run against one server: go-httpbin, plus Caddy for the three HTTP/3 tests. Real HTTP servers disagree with each other in ways that matter — compression negotiation, response framing, keep-alive limits, HTTP/2 concurrency and GOAWAY, header size limits, cache validators — and none of that disagreement is currently exercised.

This bit us once already. The HTTP/3 cancellation bug (#23) only became visible against a real HTTP/3 server with a real Alt-Svc advertisement; go-httpbin cannot produce either. Rather than accreting one server per bug, this designs the matrix deliberately.

## What the existing suite actually is

A survey of all 48 JS test files, classified by what they depend on. A few files span more than one class, so the counts total slightly above 48:

| Class | Count | Meaning |
| --- | --- | --- |
| Client-only | 37 | Tests faith's own API; the server is an interchangeable echo |
| Server-behaviour | 6 | Depends on how the server responds in ways implementations differ on |
| httpbin-specific | 4 | Depends on go-httpbin's particular endpoints or response shapes |
| No server | 4 | Needs no HTTP server at all |

Two facts follow, and together they determine the design.

**The suite is overwhelmingly client-only.** Of the 6 server-behaviour tests, 3 are the Caddy HTTP/3 tests added with #23/#29/#30 and 3 hit real internet hosts (badssl.com, cloudflare.com, test-ipv6.com). Running the existing suite against N servers would be ~48N runs that almost entirely re-test faith's API against interchangeable echoes.

**Almost every client-only test needs request *echo*.** They assert on faith's behaviour but read it out of go-httpbin's `/get`, `/headers`, `/post` response shapes. A static origin like nginx cannot echo a request back as JSON, so hosting those tests elsewhere would mean putting an echo application behind every server — turning every row into a proxy topology for no benefit.

So parameterising the existing suite is the wrong move. This design adds a **separate, purpose-built conformance suite** whose assertions are chosen because implementations differ, and which asserts on *response* characteristics — encoding, framing, protocol version, connection lifecycle, cache validators — that a static origin can produce from configuration alone.

The existing suite stays on go-httpbin, unchanged, as the client-behaviour suite.

## Goals

- Exercise the dimensions where server implementations genuinely disagree, against more than one implementation of each.
- Keep it off the critical path of every pull request.
- Make the matrix derive from declared capabilities rather than being hand-maintained, so it cannot drift from what the servers can actually do.
- Reuse the certificate and port helpers already in `test/fixtures/h3-blackhole.js`.

## Non-goals

- Parameterising the existing 48 test files. They stay on go-httpbin.
- Replacing go-httpbin. It remains the echo application for the client-behaviour suite and the backend for proxy topologies.
- Testing server correctness. The subject under test is always faith; a server is only ever a source of legitimate-but-different behaviour.
- Running on macOS or Windows. The harness spawns server binaries and binds fixed loopback ports; the behaviour under test is client-side and platform-independent.

## The matrix

### Rows: topologies

A proxy composes with an origin rather than replacing it, so rows are topologies, not server names.

| # | Topology | Unique contribution | HTTP/2 stack | HTTP/3 stack |
| --- | --- | --- | --- | --- |
| 1 | Controllable Node origin | trailers, GOAWAY at a chosen moment, deliberate framing violations | nghttp2 | — |
| 2 | Caddy | full Alt-Svc upgrade path; already in CI | Go `net/http2` | quic-go |
| 3 | nginx | own gzip module; most-deployed reverse proxy | nghttp2 | see caveat |
| 4 | Apache httpd | conservative HTTP/1 framing, 100-continue, chunked edge cases | nghttp2 (mod_http2) | — |
| 5 | HAProxy in front of go-httpbin | proxy path; connection reuse and GOAWAY under load | its own | — |
| 6 | quiche | Cloudflare's QUIC — a third HTTP/3 wire implementation | — | quiche |

### Columns: conformance dimensions

| Dimension | 1 ctrl | 2 Caddy | 3 nginx | 4 Apache | 5 HAProxy | 6 quiche |
| --- | --- | --- | --- | --- | --- | --- |
| Encoding negotiation (gzip/br/zstd, `Vary`) | yes | yes | yes | yes | yes (re-encode) | — |
| Framing: chunked vs `Content-Length` | yes | yes | yes | yes | yes | — |
| Trailers | **only here** | — | — | — | — | — |
| ALPN to h1/h2/h3 | yes | yes | yes | yes | yes | — |
| Alt-Svc / HTTP/3 upgrade | yes | yes | see caveat | — | — | n/a |
| Keep-alive limits, server-initiated close | yes (precise) | yes | yes | yes | yes | — |
| HTTP/2 concurrency and GOAWAY | yes (precise) | yes | yes | yes | **best** | — |
| Conditional requests to 304 | yes | yes | yes | yes | yes | — |
| Header size limits to 4xx/431 | yes | yes | yes | yes | yes | — |
| TLS: custom CA, client certs, versions | yes | yes | yes | yes | yes | yes |
| HTTP/3 wire behaviour (raw) | — | yes | see caveat | — | — | **yes** |

### Why these rows, and the limits of each

**HTTP/2 diversity is capped by library reuse.** faith's client uses the `h2` crate via hyper. Any server also using `h2` would test that crate against itself. Across the rows the distinct HTTP/2 implementations are nghttp2 (Node, nginx, Apache), Go's `net/http2` (Caddy), and HAProxy's own. The nginx and Apache rows therefore earn their place on *other* dimensions — gzip modules, HTTP/1 framing, keep-alive semantics, header limits — not on HTTP/2 diversity.

**Cloudflare's Pingora was considered and rejected.** It is the obvious answer to "what does a large fraction of the internet sit behind", but `pingora-core` depends on `httparse` for HTTP/1 and the `h2` crate for HTTP/2 — the same two crates faith depends on through hyper, both present in our `Cargo.lock`. It would test our own libraries against themselves. It also has no QUIC or HTTP/3 at all (roadmap, not implemented), and is a framework rather than a binary, so it would need a bespoke reverse-proxy written first. Highest cost, least distinct signal.

**quiche is HTTP/3 only.** `quiche-server` has no TCP listener, so row 6 cannot test the Alt-Svc upgrade path and must be bootstrapped with `http3.hints`, exactly as the existing HTTP/3 tests do. It buys raw HTTP/3 wire conformance against a third implementation (faith is quinn, Caddy is quic-go) and nothing else. H2O was the alternative — broader, adding a distinct HTTP/2 implementation and a second complete upgrade path — but its extra HTTP/2 coverage is the least-needed axis, its upgrade path duplicates Caddy, and it is a cmake/C/submodule build against quiche's cargo build.

**nginx HTTP/3 depends on the version available.** HTTP/3 needs nginx 1.25 or newer. Ubuntu 24.04 ships 1.24, so an `apt`-installed nginx covers HTTP/1 and HTTP/2 only. The nginx HTTP/3 cells are therefore out of scope until a newer nginx is sourced; that is a provisioning decision, not a design one, and the capability model below makes it a one-line change when it happens.

## Architecture

### Capability-driven, not hand-maintained

The table above is documentation. The runnable matrix is computed: each server declares what it can do, each test declares what it needs, and the runner skips cells whose requirements are unmet. A hand-written matrix would drift from reality the first time a server's build changed.

```
test/conformance/
  servers/            one module per topology
    controllable.js   Node http/https/http2, fully scriptable
    caddy.js          reuses the existing fixture's spawn logic
    nginx.js
    apache.js
    haproxy.js        + go-httpbin behind it
    quiche.js
  dimensions/         one module per conformance dimension
    encoding.js
    framing.js
    trailers.js
    alpn.js
    altsvc.js
    keepalive.js
    h2-goaway.js
    conditional.js
    header-limits.js
    tls.js
    h3-wire.js
  capabilities.js     the capability vocabulary, single source of truth
  run.js              computes and executes the matrix
```

Each server module exports:

```js
{
  name: "nginx",
  capabilities: new Set(["h1", "h2", "gzip", "brotli", "chunked",
                         "conditional", "headerLimits", "keepaliveLimit",
                         "goaway", "tls", "clientCerts"]),
  async start()  // -> { url, ca, close() }
}
```

Each dimension module exports tests that declare their requirements:

```js
{
  name: "encoding",
  requires: ["gzip"],
  optional: ["brotli", "zstd"],   // sub-assertions skipped individually
  run(t, server) { /* assertions against server.url */ },
}
```

`run.js` produces the cross product, drops unmet cells with a reason, and prints the realised matrix so a shrinking matrix is visible rather than silent. This is the same principle as the `CI`-set guard in the existing HTTP/3 tests: a cell that vanishes because provisioning broke must be loud.

### Shared fixtures

`test/fixtures/h3-blackhole.js` already contains `ensureCert`, `findFreePort`, `startCaddy`, `startTcpProxy` and `startUdpRelay`. The first three generalise directly; the relay is specific to HTTP/3 fault injection. Extract `ensureCert` and `findFreePort` into `test/fixtures/net.js`, have both the HTTP/3 tests and the conformance harness import them, and leave the fault-injection machinery where it is.

The certificate helper already generates a private CA plus a leaf for `localhost` and `127.0.0.1`, which is what every TLS row needs. Client certificates need one addition: a second leaf signed by the same CA, for the `clientCerts` capability.

### Provisioning

| Row | How | Cost |
| --- | --- | --- |
| Controllable Node origin | none — plain Node | free |
| Caddy | `go install`, pinned; already in `test.yml` | ~30s cached |
| nginx | `apt-get install nginx` | seconds |
| Apache httpd | `apt-get install apache2` | seconds |
| HAProxy | `apt-get install haproxy` + existing go-httpbin | seconds |
| quiche | `cargo build` from a pinned git revision; needs cmake and Go for BoringSSL, both already present on the runner | minutes, cacheable |

All pinned, for the same reason go-httpbin and Caddy are pinned in `test.yml`: an upstream behaviour change should be a deliberate bump, not a surprise CI failure.

### CI

A separate `conformance.yml`, Linux-only, single job. Not on every pull request — it provisions five servers and is minutes rather than seconds.

Triggers: pushes to `main`, a nightly schedule, and a `conformance` label on a pull request for when a change plausibly affects protocol behaviour.

`test.yml` is untouched, including the Caddy install it already does for the HTTP/3 tests.

## Error handling and failure modes

- **A server fails to start.** Fail that row loudly with the server's own log attached; do not silently skip. The existing Caddy fixture learned this the hard way — a config error surfaced as a hung run because the process was left alive on the rejection path.
- **A capability is missing.** Skip the cell, print it in the realised matrix with the reason. Distinguishable from a failure.
- **Provisioning regresses in CI.** Same guard as the HTTP/3 tests: if the environment says CI and a server binary is absent, fail rather than skip.
- **Teardown.** Every server module's `close()` must destroy live sockets rather than waiting on them; the agent under test holds pooled connections open, and a bare `server.close()` never settles. This is a fixed bug in the existing TCP proxy fixture and the same trap applies to every new row.

## Testing the harness itself

The harness is test infrastructure, so its own correctness matters and cannot be assumed:

- Each dimension module must fail when pointed at a server that violates the behaviour. The controllable Node origin exists partly to make this checkable: it can be told to emit a wrong `Content-Encoding`, omit a trailer, or send GOAWAY early, so each dimension gets a negative case.
- The realised matrix is asserted against an expected snapshot, so a cell silently disappearing fails the run.

## Decomposition

This is too large for a single implementation plan. Proposed phases, each shipping something working on its own:

1. **Harness plus the controllable origin.** `capabilities.js`, `run.js`, the extracted `net.js` fixtures, the controllable Node server, and three dimensions including trailers (which nothing else can test). Proves the capability model end to end.
2. **Caddy and nginx rows.** Caddy reuses existing spawn logic; nginx is the first genuinely foreign configuration. Adds the encoding, framing, ALPN and conditional dimensions across three rows.
3. **Apache and HAProxy rows**, plus the keep-alive, GOAWAY and header-limit dimensions where the proxy row is most valuable.
4. **quiche row** and the raw HTTP/3 wire dimension.
5. **`conformance.yml`** and README documentation of how to run it locally.

Phase 1 is the only one that risks the design being wrong, so it should land and be reviewed before the rest.

## Follow-ups

Neither is in the phases above, but one of them constrains a decision made now.

### Publishing the matrix to the README

The realised matrix is the natural source for a table in the README, so users can see which servers faith is actually verified against and on which dimensions. The format is only worth settling once the matrix has stabilised — but it shapes one decision immediately, which is why acceptance criterion 2 exists: `run.js` must emit the realised matrix as **structured data**, not only human-readable console output, so a later step can render it to Markdown or SVG without re-deriving anything.

A generated table also cannot go stale the way a hand-written one would — the same argument as computing the matrix from capabilities in the first place.

### Keeping the benchmark charts fresh

The README embeds four committed SVGs — `bench/concurrency-throughput.svg`, `latency-vs-size.svg`, `latency-by-impl.svg` and `features-rps.svg` — produced by `bench/run.mjs` and `bench/plot.mjs`. Nothing regenerates them, so they drift silently, and their captions make competitive claims that go stale with them.

The shape of the fix is a pre-release step that runs the bench, regenerates the SVGs and commits them, so a release always ships current charts. Two things need deciding first, which is why this isn't simply a workflow addition:

- **Numbers from shared CI runners are noisy.** GitHub runners have variable CPU and noisy neighbours, so absolute figures would move between runs for reasons unrelated to faith, and committing them as published fact risks presenting noise as a result. Either the bench runs somewhere dedicated, or the charts stay explicitly *relative* — comparing implementations measured within a single run, which is what they already do and is the defensible reading.
- **Regenerating every release makes the diff noisy.** Four SVGs changing on every release bloats history for movements that may be within measurement error. Committing only when something moved beyond noise is probably wanted, which requires a noise estimate to exist first.

This belongs to `bench/` rather than the conformance harness and can be done independently. It shares only the general principle: anything in the README claiming to describe current behaviour should be generated, not hand-maintained.

## Acceptance criteria

1. `node test/conformance/run.js` computes the matrix from declared capabilities, executes every satisfiable cell, and prints the realised matrix including skipped cells with reasons.
2. The realised matrix is also emitted as structured data, so it can later be rendered into the README without re-deriving it.
3. Every dimension has a negative case against the controllable origin, demonstrating the assertion can fail.
4. A server that fails to start fails its row with the server's log, and does not hang the run.
5. The existing 48-file suite and `test.yml` are unchanged.
6. Conformance runs in its own workflow, not on every pull request.
