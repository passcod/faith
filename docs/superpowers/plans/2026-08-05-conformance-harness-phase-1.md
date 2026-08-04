# Conformance Harness Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the conformance harness and prove the capability-driven matrix works end to end, using a controllable Node origin and three dimensions — trailers, framing and encoding.

**Architecture:** Servers declare what they can do, dimensions declare what they need, and a runner computes the cross product and skips unmet cells with a reason. The controllable Node origin is split into two rows (HTTP/1-only and HTTP/2-only listeners sharing route handlers) because `chunked` is an HTTP/1 concept that HTTP/2 cannot express — which makes the capability model do real work from the first task rather than waiting for nginx. The origin can also be told to misbehave, so every dimension gets a negative case proving its assertion can fail.

**Tech Stack:** Node's built-in `https` and `http2` for the origin, `zlib` for encoding, `tape` for assertions (matching the existing suite), and the existing private-CA certificate fixture.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-08-05-multi-server-conformance-matrix-design.md`. Read it before starting.
- **Work in the jj workspace `/home/felix/code/rust/faith-conformance`**, not in `/home/felix/code/rust/faith`. That is a separate working copy of the same repo; the default workspace belongs to someone else right now.
- Version control is **jj, not git**. Commit with `jj describe -m "..."` then `jj new`. Never run `git commit`, `git add` or `git checkout`.
- Indentation in this repo is **tabs**, in both Rust and JS. Match the surrounding style.
- **Do not modify any existing test file's assertions.** Task 1 refactors a fixture that three HTTP/3 test files import; those tests must keep passing unchanged.
- **Do not modify `src/`.** Phase 1 is test infrastructure only. If a dimension appears to reveal a faith bug, report it rather than fixing it here.
- No platform guards. Phase 1 spawns no external binaries and needs none; the `process.platform === "linux"` guards in the HTTP/3 tests exist because those spawn Caddy, which is not this phase's situation.
- `caddy` is on PATH and go-httpbin runs on `http://localhost:8888` in this environment; neither is needed by phase 1, but the existing suite needs httpbin.
- **`npm run build` is NOT needed** — phase 1 touches no Rust. The prebuilt `faith.linux-x64-gnu.node` in the workspace is current.

---

### Task 1: Extract the shared network fixtures

`ensureCert` and `findFreePort` currently live in the HTTP/3 fault-injection fixture. The conformance servers need both without the UDP relay machinery.

**Files:**
- Create: `test/fixtures/net.js`
- Modify: `test/fixtures/h3-blackhole.js` (remove the two functions, import and re-export them)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `ensureCert() -> { ca: Buffer, caPath: string, certPath: string, keyPath: string }`
  - `findFreePort() -> Promise<number>` — a port free for both TCP and UDP on 127.0.0.1

- [ ] **Step 1: Read the current implementations**

Read `test/fixtures/h3-blackhole.js` lines 20-90. `ensureCert` generates a private CA plus a leaf for `localhost`/`127.0.0.1` cached in tmp; `findFreePort` binds a TCP server then verifies the same port is free for UDP.

Note which test files import them, so you can confirm nothing breaks:

Run: `grep -rln "h3-blackhole" test/`
Expected: `test/http3-abort-fallback.test.js`, `test/http3-advertised-port.test.js`, `test/http3-cache-ordering.test.js`

- [ ] **Step 2: Create `test/fixtures/net.js`**

Move both functions verbatim — do not change their behaviour. Only the surrounding module scaffolding is new:

```javascript
/**
 * Network fixtures shared by the HTTP/3 tests and the conformance harness.
 *
 * Extracted from h3-blackhole.js so conformance servers can reuse the
 * certificate and port helpers without pulling in the UDP fault-injection
 * machinery, which is specific to the HTTP/3 fallback tests.
 */

const { execFileSync } = require("node:child_process");
const { existsSync, readFileSync, mkdirSync } = require("node:fs");
const dgram = require("node:dgram");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");

/**
 * Private CA plus a leaf for localhost, cached in tmp. A plain self-signed cert
 * isn't enough: rustls refuses to accept a CA certificate as an end-entity, so
 * the server presents the leaf and the client trusts the CA.
 */
function ensureCert() {
	const dir = path.join(os.tmpdir(), "faith-test-cert-v1");
	const caKeyPath = path.join(dir, "ca-key.pem");
	const caPath = path.join(dir, "ca.pem");
	const keyPath = path.join(dir, "key.pem");
	const csrPath = path.join(dir, "leaf.csr");
	const certPath = path.join(dir, "cert.pem");
	if (!existsSync(caPath) || !existsSync(keyPath) || !existsSync(certPath)) {
		mkdirSync(dir, { recursive: true });
		const ec = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1"];
		execFileSync("openssl", [
			"req", "-x509", ...ec, "-keyout", caKeyPath, "-out", caPath,
			"-days", "30", "-nodes", "-subj", "/CN=faith-test-ca",
			"-addext", "basicConstraints=critical,CA:TRUE",
		]);
		execFileSync("openssl", [
			"req", "-new", ...ec, "-keyout", keyPath, "-out", csrPath,
			"-nodes", "-subj", "/CN=localhost",
			"-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1",
			"-addext", "basicConstraints=critical,CA:FALSE",
		]);
		execFileSync("openssl", [
			"x509", "-req", "-in", csrPath, "-CA", caPath, "-CAkey", caKeyPath,
			"-CAcreateserial", "-out", certPath, "-days", "30",
			"-copy_extensions", "copyall",
		]);
	}
	return { ca: readFileSync(caPath), caPath, certPath, keyPath };
}

/** A port free for both TCP and UDP on 127.0.0.1. */
async function findFreePort() {
	for (let attempt = 0; attempt < 20; attempt++) {
		const port = await new Promise((resolve, reject) => {
			const srv = net.createServer();
			srv.once("error", reject);
			srv.listen(0, "127.0.0.1", () => {
				const { port } = srv.address();
				srv.close(() => resolve(port));
			});
		});
		const udpFree = await new Promise((resolve) => {
			const sock = dgram.createSocket("udp4");
			sock.once("error", () => resolve(false));
			sock.bind(port, "127.0.0.1", () => sock.close(() => resolve(true)));
		});
		if (udpFree) return port;
	}
	throw new Error("could not find a port free for both TCP and UDP");
}

module.exports = { ensureCert, findFreePort };
```

- [ ] **Step 3: Rewire `test/fixtures/h3-blackhole.js`**

Delete its `ensureCert` and `findFreePort` definitions and the imports that only they used (`execFileSync` is still needed by `caddyAvailable`; `dgram` is still needed by `startUdpRelay`; check each before removing). Add near the top:

```javascript
const { ensureCert, findFreePort } = require("./net.js");
```

Keep both names in its `module.exports`. The three HTTP/3 test files import them from `h3-blackhole.js`, and re-exporting means those files need no changes:

```javascript
module.exports = {
	ensureCert,
	findFreePort,
	startCaddy,
	startTcpProxy,
	startUdpRelay,
	caddyAvailable,
};
```

- [ ] **Step 4: Verify the HTTP/3 tests still pass unchanged**

Run: `npx tape test/http3-abort-fallback.test.js test/http3-advertised-port.test.js test/http3-cache-ordering.test.js`
Expected: PASS, 21 assertions total (3 + 13 + 5). No test file was edited, so any failure means the extraction changed behaviour.

- [ ] **Step 5: Commit**

```bash
jj describe -m "test: extract shared network fixtures from the HTTP/3 fixture

ensureCert and findFreePort are needed by the conformance servers, which have
no use for the UDP fault-injection machinery they currently sit beside. Moved
verbatim into test/fixtures/net.js and re-exported, so the three HTTP/3 test
files that import them need no changes.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj new
```

---

### Task 2: Capability vocabulary and the controllable origin

**Files:**
- Create: `test/conformance/capabilities.js`
- Create: `test/conformance/servers/controllable-routes.js`
- Create: `test/conformance/servers/controllable.js`
- Test: `test/conformance/servers/controllable.selftest.js`

**Interfaces:**
- Consumes: `ensureCert`, `findFreePort` from `test/fixtures/net.js` (Task 1).
- Produces:
  - `CAPABILITIES` — frozen object of known capability names
  - `assertKnownCapabilities(names: Iterable<string>, context: string)` — throws on an unknown name
  - `controllableH1` and `controllableH2`, each `{ name: string, capabilities: Set<string>, start(): Promise<{ url: string, ca: Buffer, close(): Promise<void> }> }`

- [ ] **Step 1: Write the capability vocabulary**

Create `test/conformance/capabilities.js`. A frozen vocabulary means a typo in a server or dimension fails loudly instead of silently skipping every cell:

```javascript
/**
 * The capability vocabulary.
 *
 * Servers declare what they can do; dimensions declare what they need. The
 * runner intersects the two. Keeping the names in one frozen object means a
 * typo fails loudly rather than silently skipping every cell that mentions it —
 * which would look exactly like a passing run with nothing to do.
 */

const CAPABILITIES = Object.freeze({
	// protocol versions the server will negotiate
	H1: "h1",
	H2: "h2",
	H3: "h3",
	// advertises HTTP/3 via Alt-Svc on a TCP response
	ALTSVC: "altsvc",
	// can emit response trailers
	TRAILERS: "trailers",
	// content codings it will apply
	GZIP: "gzip",
	BROTLI: "brotli",
	ZSTD: "zstd",
	// response framing it can produce. HTTP/2 has no chunked encoding, so an
	// h2-only server must not claim CHUNKED.
	CHUNKED: "chunked",
	CONTENT_LENGTH: "contentLength",
	// honours If-None-Match / If-Modified-Since with a 304
	CONDITIONAL: "conditional",
	// rejects oversized request headers with a 4xx
	HEADER_LIMITS: "headerLimits",
	// closes the connection after a configured number of requests
	KEEPALIVE_LIMIT: "keepaliveLimit",
	// can be made to send an HTTP/2 GOAWAY
	GOAWAY: "goaway",
	// TLS, and client-certificate authentication
	TLS: "tls",
	CLIENT_CERTS: "clientCerts",
	// can be instructed to misbehave, so a dimension's negative case can run
	SCRIPTABLE: "scriptable",
});

const KNOWN = new Set(Object.values(CAPABILITIES));

/** Throw if any name is outside the vocabulary. `context` names the culprit. */
function assertKnownCapabilities(names, context) {
	for (const name of names) {
		if (!KNOWN.has(name)) {
			throw new Error(
				`${context} declares unknown capability ${JSON.stringify(name)}; ` +
					`known capabilities are ${[...KNOWN].sort().join(", ")}`,
			);
		}
	}
}

module.exports = { CAPABILITIES, KNOWN, assertKnownCapabilities };
```

- [ ] **Step 2: Write the shared route handler**

Create `test/conformance/servers/controllable-routes.js`. These routes are the vocabulary the dimensions assert against, including the deliberately-wrong ones that make negative cases possible:

```javascript
/**
 * Routes for the controllable origin, shared by its HTTP/1 and HTTP/2 listeners.
 *
 * Every route is either a correct behaviour a dimension asserts, or a
 * deliberately wrong one so that dimension's negative case can prove the
 * assertion is capable of failing. A conformance test that cannot fail is
 * decoration.
 */

const zlib = require("node:zlib");

const PAYLOAD = "conformance-payload";
const TRAILER_NAME = "x-conformance-checksum";
const TRAILER_VALUE = "abc123";

function handle(req, res) {
	const url = new URL(req.url, "https://localhost");

	switch (url.pathname) {
		// --- baseline ---
		case "/hello":
			res.setHeader("content-type", "text/plain");
			res.end(PAYLOAD);
			return;

		// --- trailers ---
		case "/trailers":
			res.setHeader("content-type", "text/plain");
			res.setHeader("trailer", TRAILER_NAME);
			res.write(PAYLOAD);
			res.addTrailers({ [TRAILER_NAME]: TRAILER_VALUE });
			res.end();
			return;

		// declares a trailer in the Trailer header and then sends none: the
		// negative case for the trailers dimension
		case "/trailers/omitted":
			res.setHeader("content-type", "text/plain");
			res.setHeader("trailer", TRAILER_NAME);
			res.write(PAYLOAD);
			res.end();
			return;

		// --- framing ---
		// no content-length, so HTTP/1 must chunk this
		case "/framing/chunked":
			res.setHeader("content-type", "text/plain");
			res.write(PAYLOAD.slice(0, 5));
			res.write(PAYLOAD.slice(5));
			res.end();
			return;

		case "/framing/length":
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-length", String(Buffer.byteLength(PAYLOAD)));
			res.end(PAYLOAD);
			return;

		// --- encoding ---
		case "/encoding/gzip": {
			const body = zlib.gzipSync(Buffer.from(PAYLOAD));
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-encoding", "gzip");
			res.setHeader("vary", "accept-encoding");
			res.end(body);
			return;
		}

		// claims gzip but sends plain text: the negative case for the encoding
		// dimension, since a client that really decompresses must fail here
		case "/encoding/mislabelled":
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-encoding", "gzip");
			res.end(PAYLOAD);
			return;

		default:
			res.statusCode = 404;
			res.end("no such route");
			return;
	}
}

module.exports = { handle, PAYLOAD, TRAILER_NAME, TRAILER_VALUE };
```

- [ ] **Step 3: Write the two server entries**

Create `test/conformance/servers/controllable.js`. Two listeners rather than one, because ALPN would otherwise always pick h2 and the HTTP/1 framing routes would never be exercised:

```javascript
/**
 * The controllable origin: two Node listeners over the test CA, sharing route
 * handlers.
 *
 * Split into HTTP/1-only and HTTP/2-only rows deliberately. A single
 * `http2.createSecureServer({ allowHTTP1: true })` would advertise h2 in ALPN
 * and faith would always choose it, so the chunked-framing routes would never
 * run. Splitting also keeps the capability declarations honest: HTTP/2 has no
 * chunked transfer encoding, so the h2 row must not claim CHUNKED.
 */

const { readFileSync } = require("node:fs");
const http2 = require("node:http2");
const https = require("node:https");

const { ensureCert, findFreePort } = require("../../fixtures/net.js");
const { CAPABILITIES: C } = require("../capabilities.js");
const { handle } = require("./controllable-routes.js");

/** Destroy live sockets rather than waiting on them. */
function makeCloser(server, sockets) {
	return () =>
		new Promise((resolve) => {
			// The agent under test keeps connections pooled, so a bare
			// server.close() never settles and the caller hangs in its teardown.
			for (const socket of sockets) socket.destroy();
			sockets.clear();
			server.close(resolve);
			setTimeout(resolve, 500).unref();
		});
}

function track(server, sockets) {
	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
	server.on("secureConnection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
}

const controllableH1 = {
	name: "controllable-h1",
	capabilities: new Set([
		C.H1,
		C.TLS,
		C.TRAILERS,
		C.GZIP,
		C.CHUNKED,
		C.CONTENT_LENGTH,
		C.SCRIPTABLE,
	]),
	async start() {
		const { ca, certPath, keyPath } = ensureCert();
		const port = await findFreePort();
		const sockets = new Set();
		const server = https.createServer(
			{
				key: readFileSync(keyPath),
				cert: readFileSync(certPath),
				ALPNProtocols: ["http/1.1"],
			},
			handle,
		);
		track(server, sockets);
		await new Promise((resolve, reject) => {
			server.once("error", reject);
			server.listen(port, "127.0.0.1", resolve);
		});
		return {
			url: `https://localhost:${port}`,
			ca,
			close: makeCloser(server, sockets),
		};
	},
};

const controllableH2 = {
	name: "controllable-h2",
	capabilities: new Set([
		C.H2,
		C.TLS,
		C.TRAILERS,
		C.GZIP,
		C.CONTENT_LENGTH,
		C.SCRIPTABLE,
	]),
	async start() {
		const { ca, certPath, keyPath } = ensureCert();
		const port = await findFreePort();
		const sockets = new Set();
		const server = http2.createSecureServer(
			{
				key: readFileSync(keyPath),
				cert: readFileSync(certPath),
				allowHTTP1: false,
			},
			handle,
		);
		track(server, sockets);
		await new Promise((resolve, reject) => {
			server.once("error", reject);
			server.listen(port, "127.0.0.1", resolve);
		});
		return {
			url: `https://localhost:${port}`,
			ca,
			close: makeCloser(server, sockets),
		};
	},
};

module.exports = { controllableH1, controllableH2 };
```

- [ ] **Step 4: Write the self-test**

Create `test/conformance/servers/controllable.selftest.js`. Named `.selftest.js`, not `.test.js`, so `npm run test:only`'s `test/*.test.js` glob does not pick it up:

```javascript
/**
 * The controllable origin is test infrastructure, so it needs its own test:
 * every dimension's verdict depends on it serving what it claims.
 */

const test = require("tape");
const { controllableH1, controllableH2 } = require("./controllable.js");
const { assertKnownCapabilities } = require("../capabilities.js");
const { PAYLOAD } = require("./controllable-routes.js");

const { Agent } = require("../../../index.js");
const { fetch } = require("../../../wrapper.js");

for (const server of [controllableH1, controllableH2]) {
	test(`controllable origin: ${server.name} serves and negotiates`, async (t) => {
		assertKnownCapabilities(server.capabilities, server.name);

		const running = await server.start();
		const agent = new Agent({
			tls: { extraRoots: [running.ca] },
			dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
			http3: { upgradeEnabled: false },
		});

		try {
			const res = await fetch(`${running.url}/hello`, { agent, timeout: 10000 });
			const body = await res.text();
			t.equal(res.status, 200, "serves the baseline route");
			t.equal(body, PAYLOAD, "and the expected payload");
			t.equal(
				res.version,
				server.name === "controllable-h1" ? "HTTP/1.1" : "HTTP/2.0",
				"negotiates exactly the protocol the row claims",
			);
		} finally {
			await running.close();
			t.end();
		}
	});
}
```

- [ ] **Step 5: Run the self-test**

Run: `npx tape test/conformance/servers/controllable.selftest.js`
Expected: PASS, 6 assertions (3 per row). If the h1 row reports `HTTP/2.0`, `ALPNProtocols` is not being honoured; if the h2 row fails to connect, `allowHTTP1: false` is rejecting the handshake.

- [ ] **Step 6: Commit**

```bash
jj describe -m "test: add the capability vocabulary and controllable origin

Servers declare what they can do and dimensions declare what they need, so the
names live in one frozen vocabulary: a typo then fails loudly instead of
silently skipping every cell that mentions it, which would look identical to a
passing run.

The origin is two listeners, not one. A single http2 server with allowHTTP1
advertises h2 in ALPN and faith always picks it, so the chunked-framing routes
would never run; splitting also keeps the declarations honest, since HTTP/2 has
no chunked encoding and the h2 row must not claim it.

Routes include deliberately wrong ones -- a declared trailer that is never sent,
a body labelled gzip that isn't -- so each dimension's negative case can prove
its assertion is able to fail.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj new
```

---

### Task 3: The trailers dimension

Trailers are the one dimension no real server in the matrix can test, which is why they come first.

**Files:**
- Create: `test/conformance/dimensions/trailers.js`
- Test: exercised via Task 4's runner; verified directly in this task with a throwaway harness

**Interfaces:**
- Consumes: `CAPABILITIES` from `test/conformance/capabilities.js`; a running server `{ url, ca, close() }` from Task 2.
- Produces: a dimension module `{ name, requires: string[], run(t, ctx), negative(t, ctx) }` where `ctx` is `{ url, agent }`.

- [ ] **Step 1: Write the dimension**

Create `test/conformance/dimensions/trailers.js`:

```javascript
/**
 * Response trailers.
 *
 * No real server in the matrix can emit trailers on demand, so this dimension
 * only ever runs against the controllable origin — which is precisely why it is
 * worth having: nothing else in faith's test suite covers trailers at all.
 */

const { CAPABILITIES: C } = require("../capabilities.js");

module.exports = {
	name: "trailers",
	requires: [C.TRAILERS],

	async run(t, { url, agent }) {
		const res = await fetchWith(agent, `${url}/trailers`);

		// Consume the body FIRST. faith resolves `trailers` only once the body
		// stream completes: the native side polls a NotYet state and the value is
		// set either by a trailers frame or by a sentinel appended after the last
		// body chunk. Awaiting trailers before draining the body therefore spins
		// forever rather than erroring.
		const body = await res.text();
		t.equal(body, "conformance-payload", "body arrives ahead of the trailers");

		const trailers = await res.trailers;
		t.ok(trailers, "trailers are exposed once the body is consumed");
		t.equal(
			trailers && trailers.get("x-conformance-checksum"),
			"abc123",
			"and carry the value the server sent",
		);
	},

	async negative(t, { url, agent }) {
		// The server declares a trailer in the Trailer header and then sends none.
		// A client that genuinely reads trailers must report their absence rather
		// than inventing them from the declaration.
		const res = await fetchWith(agent, `${url}/trailers/omitted`);
		await res.text();
		const trailers = await res.trailers;
		const value = trailers && trailers.get("x-conformance-checksum");
		t.notOk(
			value,
			"a declared-but-unsent trailer is absent, not fabricated from the Trailer header",
		);
	},
};

function fetchWith(agent, target) {
	const { fetch } = require("../../../wrapper.js");
	return fetch(target, { agent, timeout: 10000 });
}
```

- [ ] **Step 2: Verify it passes and its negative case is meaningful**

Write this throwaway file at `/tmp/trailers-check.js` — it is scaffolding, not part of the deliverable, and must be deleted in step 4:

```javascript
const test = require("tape");
const { controllableH1 } = require("/home/felix/code/rust/faith-conformance/test/conformance/servers/controllable.js");
const dim = require("/home/felix/code/rust/faith-conformance/test/conformance/dimensions/trailers.js");
const { Agent } = require("/home/felix/code/rust/faith-conformance/index.js");

test("trailers dimension against the controllable origin", async (t) => {
	const running = await controllableH1.start();
	const agent = new Agent({
		tls: { extraRoots: [running.ca] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
		http3: { upgradeEnabled: false },
	});
	try {
		await dim.run(t, { url: running.url, agent });
		await dim.negative(t, { url: running.url, agent });
	} finally {
		await running.close();
		t.end();
	}
});
```

Run: `npx tape /tmp/trailers-check.js`
Expected: PASS, 4 assertions.

If it hangs instead of failing, the body was not consumed before awaiting `res.trailers` — re-read the comment in `run`.

- [ ] **Step 3: Confirm the positive assertion can fail**

Temporarily change `run`'s expected trailer value from `"abc123"` to `"wrong"`, re-run the check, and confirm that assertion fails. Then change it back and re-run to confirm it passes again. A dimension that cannot fail tells you nothing, and this is the cheapest possible proof that it can.

Run: `npx tape /tmp/trailers-check.js`
Expected: first run FAIL on the trailer-value assertion; after reverting, PASS 4/4.

- [ ] **Step 4: Delete the scaffolding and commit**

```bash
rm /tmp/trailers-check.js
jj describe -m "test: add the trailers conformance dimension

Trailers are the one dimension no real server in the matrix can produce on
demand, and nothing in faith's suite covered them at all.

Documents the contract that costs an hour to rediscover: faith resolves
response.trailers only once the body stream completes, because the native side
polls a NotYet state that is resolved either by a trailers frame or by a
sentinel appended after the last body chunk. Awaiting trailers before draining
the body spins forever rather than erroring.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj new
```

---

### Task 4: The runner

**Files:**
- Create: `test/conformance/run.js`
- Create: `test/conformance/expected-matrix.json`
- Modify: `package.json` (add a `test:conformance` script)
- Modify: `.gitignore` (ignore the emitted `test/conformance/matrix.json`)

**Interfaces:**
- Consumes: server modules from Task 2; the trailers dimension from Task 3; `assertKnownCapabilities` from Task 2.
- Produces: `npm run test:conformance`, and `test/conformance/matrix.json` as structured output.

- [ ] **Step 1: Write the runner**

Create `test/conformance/run.js`:

```javascript
/**
 * Computes and runs the conformance matrix.
 *
 * The matrix is derived, not written down: each server declares its
 * capabilities, each dimension declares its requirements, and a cell runs only
 * when the requirements are met. A hand-maintained matrix would drift from what
 * the servers can actually do the first time a build changed.
 *
 * Emits `matrix.json` alongside the human-readable output, so the realised
 * matrix can later be rendered into the README without re-deriving it.
 */

const test = require("tape");
const { writeFileSync } = require("node:fs");
const path = require("node:path");

const { CAPABILITIES: C, assertKnownCapabilities } = require("./capabilities.js");
const { controllableH1, controllableH2 } = require("./servers/controllable.js");

const { Agent } = require("../../index.js");

const SERVERS = [controllableH1, controllableH2];
const DIMENSIONS = [require("./dimensions/trailers.js")];

const EXPECTED = require("./expected-matrix.json");

function planCells() {
	const cells = [];
	for (const server of SERVERS) {
		assertKnownCapabilities(server.capabilities, `server ${server.name}`);
		for (const dimension of DIMENSIONS) {
			assertKnownCapabilities(dimension.requires, `dimension ${dimension.name}`);
			const missing = dimension.requires.filter((c) => !server.capabilities.has(c));
			cells.push({
				server: server.name,
				dimension: dimension.name,
				status: missing.length === 0 ? "run" : "skip",
				reason: missing.length === 0 ? null : `lacks ${missing.join(", ")}`,
			});
		}
	}
	return cells;
}

async function main() {
	const cells = planCells();

	// Structured output first, so it exists even if a cell later fails.
	const out = path.join(__dirname, "matrix.json");
	writeFileSync(out, `${JSON.stringify({ cells }, null, "\t")}\n`);

	test("conformance: realised matrix matches the expected one", (t) => {
		// A cell silently disappearing -- because a capability declaration
		// changed, or a server stopped starting -- looks identical to a clean run
		// unless the shape itself is asserted.
		t.deepEqual(
			cells.map(({ server, dimension, status }) => ({ server, dimension, status })),
			EXPECTED.cells,
			"no cell appeared or vanished unnoticed",
		);
		t.end();
	});

	for (const cell of cells) {
		if (cell.status === "skip") {
			test(`${cell.server} / ${cell.dimension}`, (t) => {
				t.pass(`skipped: ${cell.reason}`);
				t.end();
			});
			continue;
		}

		const server = SERVERS.find((s) => s.name === cell.server);
		const dimension = DIMENSIONS.find((d) => d.name === cell.dimension);

		test(`${cell.server} / ${cell.dimension}`, async (t) => {
			let running;
			try {
				running = await server.start();
			} catch (err) {
				// Loud, not skipped: a server that will not start is a failure of
				// the row, and its own log is the only useful diagnostic.
				t.fail(`${cell.server} failed to start: ${err.message}`);
				t.end();
				return;
			}

			const agent = new Agent({
				tls: { extraRoots: [running.ca] },
				dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
				http3: { upgradeEnabled: false },
			});
			const ctx = { url: running.url, agent };

			try {
				await dimension.run(t, ctx);
				if (dimension.negative && server.capabilities.has(C.SCRIPTABLE)) {
					await dimension.negative(t, ctx);
				}
			} finally {
				await running.close();
				t.end();
			}
		});
	}
}

main();
```

- [ ] **Step 2: Write the expected matrix**

Create `test/conformance/expected-matrix.json`. Both controllable rows declare `trailers`, so both cells run:

```json
{
	"cells": [
		{ "server": "controllable-h1", "dimension": "trailers", "status": "run" },
		{ "server": "controllable-h2", "dimension": "trailers", "status": "run" }
	]
}
```

- [ ] **Step 3: Add the script and the ignore**

In `package.json`, add to `scripts`, after `test:integration`:

```json
"test:conformance": "node test/conformance/run.js"
```

In `.gitignore`, add:

```
test/conformance/matrix.json
```

- [ ] **Step 4: Run it**

Run: `npm run test:conformance`
Expected: PASS. One assertion for the matrix shape, plus 4 per running cell (3 positive + 1 negative) across two rows: 9 assertions total.

Then confirm the structured output exists and is usable:

Run: `cat test/conformance/matrix.json`
Expected: JSON with two cells, both `"status": "run"`.

- [ ] **Step 5: Confirm a vanishing cell is caught**

Temporarily remove `C.TRAILERS` from `controllableH2`'s capability set and re-run.

Run: `npm run test:conformance`
Expected: the matrix-shape assertion FAILS, because that cell became a skip. This is the guard doing its job. Restore the capability and re-run to confirm PASS.

- [ ] **Step 6: Commit**

```bash
jj describe -m "test: add the conformance matrix runner

The matrix is derived from declared capabilities rather than written down, so
it cannot drift from what the servers can actually do. Cells whose requirements
are unmet are skipped with a reason; a server that fails to start fails its row
loudly, since silently skipping it is indistinguishable from a clean run.

The realised matrix is asserted against a committed expectation, so a cell
appearing or vanishing -- from a changed capability declaration, or a server
that stopped starting -- fails instead of passing quietly. It is also written
to matrix.json so it can later be rendered into the README without being
re-derived.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj new
```

---

### Task 5: The framing and encoding dimensions

Two more dimensions, chosen because one of them must be skipped on one row — which is what proves the capability model is load-bearing rather than decorative.

**Files:**
- Create: `test/conformance/dimensions/framing.js`
- Create: `test/conformance/dimensions/encoding.js`
- Modify: `test/conformance/run.js` (register both)
- Modify: `test/conformance/expected-matrix.json` (six cells, one of them a skip)

**Interfaces:**
- Consumes: `CAPABILITIES`; the `{ url, agent }` context from Task 4.
- Produces: two more dimension modules of the same shape as Task 3's.

- [ ] **Step 1: Write the framing dimension**

Create `test/conformance/dimensions/framing.js`. It requires `chunked`, which the h2 row does not have — so this cell is expected to skip there:

```javascript
/**
 * Response framing: chunked transfer encoding versus Content-Length.
 *
 * Requires CHUNKED, which HTTP/2 cannot provide — HTTP/2 frames bodies and has
 * no chunked encoding — so this dimension skips the h2 row. That skip is the
 * point: it demonstrates the capability model excluding a cell that genuinely
 * cannot run, rather than every cell running everywhere.
 *
 * This dimension has no separate negative case because its two routes are each
 * other's control: if Content-Length detection were stuck absent the sized
 * assertion fails, and if stuck present the chunked assertion fails. Either
 * direction of breakage is caught, so a third contrived route would add nothing.
 */

const { CAPABILITIES: C } = require("../capabilities.js");

module.exports = {
	name: "framing",
	requires: [C.CHUNKED, C.CONTENT_LENGTH],

	async run(t, { url, agent }) {
		const chunked = await fetchWith(agent, `${url}/framing/chunked`);
		const chunkedBody = await chunked.text();
		t.equal(chunkedBody, "conformance-payload", "reassembles a chunked body");
		t.notOk(
			chunked.headers.get("content-length"),
			"a chunked response carries no Content-Length",
		);

		const sized = await fetchWith(agent, `${url}/framing/length`);
		const sizedBody = await sized.text();
		t.equal(sizedBody, "conformance-payload", "reads a Content-Length body");
		t.equal(
			sized.headers.get("content-length"),
			String(Buffer.byteLength("conformance-payload")),
			"and reports the declared length",
		);
	},
};

function fetchWith(agent, target) {
	const { fetch } = require("../../../wrapper.js");
	return fetch(target, { agent, timeout: 10000 });
}
```

- [ ] **Step 2: Write the encoding dimension**

Create `test/conformance/dimensions/encoding.js`:

```javascript
/**
 * Content coding: does the client actually decompress what the server labelled?
 */

const { CAPABILITIES: C } = require("../capabilities.js");

module.exports = {
	name: "encoding",
	requires: [C.GZIP],

	async run(t, { url, agent }) {
		const res = await fetchWith(agent, `${url}/encoding/gzip`);
		const body = await res.text();
		t.equal(body, "conformance-payload", "transparently decompresses gzip");
		t.notOk(
			res.headers.get("content-encoding"),
			"and strips Content-Encoding once decoded, so the body matches the header",
		);
	},

	async negative(t, { url, agent }) {
		// Labelled gzip, sent as plain text. A client that really decompresses
		// must fail; one that passes the bytes through would happily return them.
		let failed = false;
		try {
			const res = await fetchWith(agent, `${url}/encoding/mislabelled`);
			await res.text();
		} catch {
			failed = true;
		}
		t.ok(failed, "a body mislabelled as gzip is rejected rather than passed through");
	},
};

function fetchWith(agent, target) {
	const { fetch } = require("../../../wrapper.js");
	return fetch(target, { agent, timeout: 10000 });
}
```

- [ ] **Step 3: Register both in the runner**

In `test/conformance/run.js`, replace the `DIMENSIONS` line:

```javascript
const DIMENSIONS = [
	require("./dimensions/trailers.js"),
	require("./dimensions/framing.js"),
	require("./dimensions/encoding.js"),
];
```

- [ ] **Step 4: Update the expected matrix**

Replace `test/conformance/expected-matrix.json` entirely. Cells are ordered server-major, matching the loop order in `planCells`:

```json
{
	"cells": [
		{ "server": "controllable-h1", "dimension": "trailers", "status": "run" },
		{ "server": "controllable-h1", "dimension": "framing", "status": "run" },
		{ "server": "controllable-h1", "dimension": "encoding", "status": "run" },
		{ "server": "controllable-h2", "dimension": "trailers", "status": "run" },
		{ "server": "controllable-h2", "dimension": "framing", "status": "skip" },
		{ "server": "controllable-h2", "dimension": "encoding", "status": "run" }
	]
}
```

- [ ] **Step 5: Run the full matrix**

Run: `npm run test:conformance`
Expected: PASS. The `controllable-h2 / framing` cell reports `skipped: lacks chunked`, and `matrix.json` records it as a skip with that reason.

If the h2 framing cell *runs*, the h2 row is wrongly claiming `CHUNKED`; if the h1 encoding negative case fails, faith is passing a mislabelled gzip body through rather than rejecting it — report that rather than adjusting the assertion, since it would be a real finding about faith.

- [ ] **Step 6: Verify nothing else regressed**

Run: `HTTPBIN_URL=http://localhost:8888 npm run test:only`
Expected: PASS, same count as before this plan started. The conformance harness lives outside the `test/*.test.js` glob, so this confirms it did not leak into the main suite.

- [ ] **Step 7: Commit and move the bookmark**

```bash
jj describe -m "test: add the framing and encoding conformance dimensions

Framing requires chunked transfer encoding, which HTTP/2 cannot provide, so
that cell skips on the h2 row. The skip is the point: it shows the capability
model excluding a cell that genuinely cannot run, rather than every dimension
running everywhere and the model being decorative.

Encoding's negative case sends a body labelled gzip that isn't, which a client
that really decompresses has to reject.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
jj new
jj bookmark set claude/conformance-matrix -r @-
```

---

## Verification before opening the PR

- [ ] `npm run test:conformance` passes, with exactly one skipped cell (`controllable-h2 / framing`).
- [ ] `HTTPBIN_URL=http://localhost:8888 npm run test:only` passes with an unchanged count.
- [ ] `npx tape test/http3-abort-fallback.test.js test/http3-advertised-port.test.js test/http3-cache-ordering.test.js` passes 21/21 — Task 1's extraction did not change behaviour.
- [ ] `test/conformance/matrix.json` is git-ignored and not committed.
- [ ] `src/` is untouched: `jj diff --stat` lists no file under `src/`.
- [ ] No `.test.js` file was added under `test/conformance/` — the harness must not be picked up by the main suite's glob.
- [ ] No scaffolding left behind: `/tmp/trailers-check.js` is deleted, and no capability declaration is still commented out from a step-5 experiment.

## Acceptance criteria (from the spec, scoped to phase 1)

1. `npm run test:conformance` computes the matrix from declared capabilities, runs every satisfiable cell, and reports skipped cells with a reason.
2. The realised matrix is emitted as structured data at `test/conformance/matrix.json`.
3. Each of the three dimensions has been shown capable of failing — trailers and encoding via an explicit negative case, framing via its two routes acting as each other's control (both directions of breakage are caught), and trailers additionally by the deliberate mutation in Task 3 Step 3.
4. A server that fails to start fails its row with a message, and does not hang the run.
5. The existing suite and `test.yml` are unchanged.
