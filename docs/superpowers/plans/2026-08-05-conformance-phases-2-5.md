# Conformance matrix, phases 2–5

Implementation plan for the rest of the harness. Phase 1 (merged as #32) built the
runner, the capability model, the controllable Node origin, and the trailers,
framing and encoding dimensions. The design and the phase decomposition live on
issue #25.

Goal: five more rows (Caddy, nginx, Apache, HAProxy, quiche), six more dimensions,
and a workflow that runs the matrix and renders it.

## Decisions this plan settles

Five things phase 1 did not have to answer, because it had one server family.

**1. The origin contract moves out of the server module.** Dimensions currently
import `PAYLOAD` and the trailer names from `servers/controllable-routes.js` — a
specific server. With more than one implementation, the paths and payloads are a
contract every row must satisfy, so they belong in `contract.js`, and each server
implements it. Each route is declared alongside the capability that makes serving
it mandatory, which turns "declares GZIP but has no `/encoding/gzip`" into a
selftest failure instead of a confusing cell failure.

**2. Availability is orthogonal to capability.** A row whose binary is not
installed must not change the computed matrix: `expected-matrix.json` is derived
from declarations alone, so it stays identical on every machine, and that is what
makes it a usable guard. Absence shows up in the *outcomes* instead, as
`outcome: "unavailable"`, and `CONFORMANCE_REQUIRE_ALL=1` — set in CI — turns any
unavailable row into a failure. A dev without nginx gets a green run and a printed
list of what did not run; CI cannot silently lose a row.

**3. Negatives are gated per dimension, not by one `SCRIPTABLE` flag.** nginx can
be configured to mislabel a content coding but can never emit trailers; a single
flag cannot express that. Each dimension declares `negativeRequires`, and the
runner intersects it the same way it does `requires`.

**4. Static rows get their behaviour from configuration; the proxy row gets it
from the origin.** Caddy, nginx and Apache serve a generated file tree and use
their own compression, ETag and framing. HAProxy sits in front of the controllable
HTTP/1 origin, so the proxy row exercises pass-through of the awkward things
(chunked bodies, trailers) rather than re-testing a static file server. This
deviates from #25's "HAProxy in front of go-httpbin": go-httpbin does not serve
the contract, and the controllable origin does.

**5. Chunked framing is a proxy behaviour, not a static-file one.** A static file
server knows the length and sends `Content-Length`; these servers only produce
chunked as a side effect of on-the-fly compression, which would make the framing
assertions secretly about gzip. So Caddy, nginx and Apache declare
`CONTENT_LENGTH` and not `CHUNKED`, framing skips on those rows, and it runs on
the HAProxy row — where chunked pass-through is the interesting case anyway.

## Two changes to the order below

**The workflow comes before the remaining rows.** Nothing runs the conformance
suite in CI today — `npm run test` does not include it — so every row added before
`conformance.yml` exists is verified only on whatever machine happened to run it.
Phase 5's workflow therefore lands with phase 2, and Apache, HAProxy and quiche
arrive into a matrix that CI is already running. The README table stays last, since
it wants the finished matrix.

**Task 8 moves to phase 4.** `makeAgent` exists for the Alt-Svc dimension, which is
in phase 4; building it in phase 2 would leave a knob nothing calls for two phases.

## Global constraints

- Every binary is pinned, matching how go-httpbin and Caddy already are: apt for
  nginx/Apache/HAProxy, `go install` for Caddy (v2.11.4, as in `test.yml`),
  `cargo build` at a fixed revision for quiche.
- Locate binaries and module directories at runtime. Arch is `httpd` with modules
  in `/usr/lib/httpd/modules`; Ubuntu is `apache2` with
  `/usr/lib/apache2/modules`. Hardcoding either makes the row a local-only or
  CI-only test.
- Servers bind `127.0.0.1` on a port from `findFreePort()`, and use the shared
  test CA from `test/fixtures/net.js`. No server writes outside its own temp dir.
- Every dimension has a negative case, or a header comment saying why its positive
  assertions already fail in both directions.
- New capability names go in the frozen vocabulary in `capabilities.js`.

---

## Phase 2 — the contract, Caddy and nginx

### Task 1: Extract the origin contract

**Files:** create `test/conformance/contract.js`; modify
`servers/controllable-routes.js`, `dimensions/{trailers,framing,encoding}.js`,
`servers/controllable.selftest.js`.

`contract.js` exports `PAYLOAD`, `TRAILER_NAME`, `TRAILER_VALUE`, and `ROUTES`: a
list of `{ path, requires, negative }` entries covering every path a dimension
fetches. `requires` is the capability that obliges a server to serve that path;
`negative` marks the deliberately-wrong routes.

`controllable-routes.js` re-exports the payload constants so it stays the one
place its own routes are defined, and imports them rather than declaring them.
Dimensions import from `contract.js`.

### Task 2: Contract enforcement in server selftests

**Files:** create `test/conformance/servers/contract-check.js`; modify
`controllable.selftest.js`.

`assertServesContract(t, { url, agent, capabilities })` walks `ROUTES`, selects
those whose `requires` the server declares, fetches each, and asserts a non-404
status. Returns the number of assertions made so callers can declare counts.

### Task 3: The static file tree

**Files:** create `test/conformance/servers/static-tree.js`.

`buildStaticTree()` writes a temp directory the configured servers serve:
`hello`, `framing/length`, `encoding/gzip`, `encoding/mislabelled`,
`conditional/etag`, each containing `PAYLOAD`. Returns `{ dir, cleanup }`.
Deterministic mtimes, so `Last-Modified` and any ETag derived from it are stable
across runs.

### Task 4: Availability and the `unavailable` outcome

**Files:** modify `run.js`, `capabilities.js`.

Server modules gain an optional `available()` returning a boolean — for
configured servers, "is the binary on PATH", reusing the `execFileSync` probe
shape already in `h3-blackhole.js`. `run.js` checks it once per server before the
cell loop:

- available: unchanged behaviour.
- not available, `CONFORMANCE_REQUIRE_ALL` unset: the cell passes with a
  `t.pass("unavailable: …")` and serialises as `outcome: "unavailable"`.
- not available, `CONFORMANCE_REQUIRE_ALL` set: `t.fail`.

`planCells()` does not consult `available()` — decision 2.

### Task 5: The Caddy row

**Files:** create `test/conformance/servers/caddy.js`,
`test/conformance/servers/caddy.selftest.js`; modify
`test/fixtures/h3-blackhole.js`.

Promote `startCaddy` and `caddyAvailable` from `h3-blackhole.js` into
`test/fixtures/caddy.js`, re-exported from `h3-blackhole.js` the way `net.js`
already is, then extend `startCaddy` to take a route map so it can serve the
static tree with `file_server`, `encode gzip`, and a mislabelled-encoding route,
instead of only a single `respond`.

Caddy declares `H1, H2, H3, ALTSVC, ALPN_MULTI, TLS, GZIP, CONTENT_LENGTH,
CONDITIONAL, SCRIPTABLE`. `expectVersion` is `HTTP/2.0`.

### Task 6: The nginx row

**Files:** create `test/conformance/servers/nginx.js`,
`test/conformance/servers/nginx.selftest.js`.

Generate `nginx.conf` into a temp prefix and spawn `nginx -p <prefix> -c
<conf> -g "daemon off;"`. Needs `error_log`/`access_log`/`pid`/`client_body_temp_path`
inside the prefix, or nginx writes to its build-time paths and fails
unprivileged. `gzip on; gzip_types text/plain; gzip_min_length 1;` for the
encoding route — the default `gzip_min_length` is 20 and the payload is 20 bytes,
so this is load-bearing. The mislabelled route is `add_header content-encoding
gzip` on a plain file.

nginx declares `H1, H2, ALPN_MULTI, TLS, GZIP, CONTENT_LENGTH, CONDITIONAL,
SCRIPTABLE`. Not `H3`: Ubuntu 24.04 ships nginx 1.24 and HTTP/3 needs 1.25+.

### Task 7: The conditional and ALPN dimensions

**Files:** create `dimensions/conditional.js`, `dimensions/alpn.js`; modify
`capabilities.js`, `run.js`, `expected-matrix.json`.

`conditional` (requires `CONDITIONAL`): GET, capture `ETag`, re-request with
`If-None-Match` and assert 304 with an empty body; then re-request with a bogus
validator and assert 200 with the payload. The bogus case is what distinguishes
real validation from a server that always answers 304.

`alpn` (requires new `ALPN_MULTI`): the row offers both `http/1.1` and `h2`, so
assert faith negotiates HTTP/2.0. Distinct from the runner's `expectVersion`
probe, which asserts what a single-protocol row must produce; this asserts a
*preference* when the server offers a choice.

---

## Phase 3 — Apache, HAProxy, and the connection-lifecycle dimensions

### Task 9: The Apache row

**Files:** create `servers/apache.js`, `servers/apache.selftest.js`.

Generate `httpd.conf` into a temp prefix. `LoadModule` lines need the real module
directory, so resolve it from the binary: try `apachectl -V`'s `HTTPD_ROOT`, then
the known Arch and Debian paths, and fail with all candidates listed. Modules:
`mpm_event`, `authz_core`, `mime`, `deflate`, `headers`, `ssl`, `http2`, `unixd`,
`log_config`, `dir`, `alias`.

Declares `H1, H2, ALPN_MULTI, TLS, GZIP, CONTENT_LENGTH, CONDITIONAL,
HEADER_LIMITS, KEEPALIVE_LIMIT, SCRIPTABLE`. `MaxKeepAliveRequests 2` and
`LimitRequestFieldSize` give the last two, and both are exactly the conservative
HTTP/1 semantics this row is here for.

### Task 10: The HAProxy row

**Files:** create `servers/haproxy.js`, `servers/haproxy.selftest.js`.

Generate `haproxy.cfg` with a TLS frontend (`ssl crt <combined.pem>`, `alpn
h2,http/1.1`) and a backend pointing at a controllable HTTP/1 origin the row
starts itself. Needs a combined cert+key PEM, which `net.js` does not currently
produce — add `ensureCombinedCert()` there.

`start()` returns a `close` that tears down both the proxy and the origin behind
it, in that order.

Declares `H1, H2, ALPN_MULTI, TLS, CHUNKED, CONTENT_LENGTH, TRAILERS, GOAWAY,
KEEPALIVE_LIMIT`. `TRAILERS` and `CHUNKED` come from the origin behind it, which
is the point of the row: trailers and chunked bodies surviving a proxy hop.

### Task 11: keepalive, goaway and header-limit dimensions

**Files:** create `dimensions/keepalive.js`, `dimensions/goaway.js`,
`dimensions/header-limits.js`; modify `capabilities.js`, `expected-matrix.json`,
`servers/controllable-routes.js`.

`keepalive` (requires `KEEPALIVE_LIMIT`): issue more requests than the server's
limit and assert every one succeeds with the payload — a client that mishandles a
server-initiated close fails here, and the assertion is on the responses rather
than on socket counts, which are not observable through faith's API.

`goaway` (requires `GOAWAY`, `H2`): a route that makes the server send GOAWAY;
assert the in-flight request completes and a subsequent request succeeds on a new
connection.

`header-limits` (requires `HEADER_LIMITS`): send a request header past the
server's configured limit and assert a 4xx arrives rather than a hang or a
transport error. The controllable origin gets `maxHeaderSize` set low enough to
declare this too.

---

## Phase 4 — quiche

### Task 12: Provision quiche

**Files:** create `test/conformance/servers/quiche.js`,
`servers/quiche.selftest.js`, `scripts/provision-quiche.sh`.

Build `quiche-server` from a pinned revision into a gitignored cache dir, skipped
when the binary is already there. Needs cmake and Go for BoringSSL, both already
on the runner.

quiche-server is HTTP/3 only — no TCP listener — so this row cannot advertise
Alt-Svc and the agent must be told to speak HTTP/3 directly. Declares `H3, TLS,
CONTENT_LENGTH`, and `expectVersion` is `HTTP/3.0`.

### Task 13: The HTTP/3 wire dimension

**Files:** create `dimensions/h3.js`; modify `capabilities.js`,
`expected-matrix.json`.

Requires `H3`: a GET over HTTP/3 returning the payload, asserting
`res.version === "HTTP/3.0"` — a third QUIC implementation behind the same
assertions the TCP rows run.

### Task 14: The Alt-Svc upgrade dimension, and dimension-scoped agents

**Files:** create `dimensions/altsvc.js`; modify `run.js`.

The runner builds one agent per cell with `http3.upgradeEnabled: false`. This
dimension needs upgrades on, so the cell context gains `makeAgent(overrides)`
returning an agent with the row's CA and DNS overrides merged with the caller's.
`agent` stays as the default, so no existing dimension changes.

Requires `ALTSVC`: assert the first response advertises Alt-Svc over TCP and that a
subsequent request negotiates HTTP/3. Runs on the Caddy row. This is the path #23
broke, so it belongs in the matrix rather than only in the regression tests.

---

## Phase 5 — the workflow and the rendered matrix

### Task 15: `matrix.md`

**Files:** create `test/conformance/render.js`; modify `package.json`,
`test/conformance/README.md`, `.gitignore`.

Read `matrix.json`, emit `matrix.md`: dimensions as rows, servers as columns,
cells as pass/fail/skipped/unavailable, with skip reasons as footnotes. Refuses
to render `kind: "planned"` — a table of what was *intended* is indistinguishable
from a table of what passed, and only one of those belongs in a README.

### Task 16: `conformance.yml`

**Files:** create `.github/workflows/conformance.yml`.

Linux only. Triggers: push to `main`, a nightly schedule, and the `conformance`
label. Provisions all five servers, runs `npm run test:conformance` with
`CONFORMANCE_REQUIRE_ALL=1`, and uploads `matrix.json` and `matrix.md` with
`if: always()` — decision 2 means a missing server fails the job, and the
artifact is the only way the file survives the runner.

### Task 17: The README table

**Files:** modify `README.md`, `.github/workflows/conformance.yml`.

Insert the rendered table between markers in the top-level README. The workflow
regenerates it on `main` and fails if it is stale, so the committed table cannot
drift from what the matrix actually does. This is the same "pre-release step"
shape the perf charts want, so keep the marker convention reusable.
