const test = require("tape");
const os = require("os");
const path = require("path");
const http2 = require("node:http2");
const { readFileSync } = require("node:fs");
const { fetch, Agent } = require("../wrapper.js");
const { url, hostname } = require("./helpers.js");
const { createConnectionTracker } = require("./fixtures/connection-tracker.js");
const { ensureCert } = require("./fixtures/net.js");

// A connection returns to the pool a runtime turn after the body resolves, so a
// networkChanged() issued immediately can land before the connection it is meant
// to drop is even pooled. Yielding a few turns first keeps the assertions about
// what got dropped deterministic (same reason as connection-reuse.test.js).
async function settlePool() {
	for (let i = 0; i < 5; i++) {
		await new Promise((resolve) => setTimeout(resolve, 2));
	}
}

test("networkChanged drops pooled connections", async (t) => {
	t.plan(3);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		await r1.text();
		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		t.equal(
			tracker.stats().totalConnections,
			1,
			"the second request reuses the pooled connection",
		);

		await settlePool();
		agent.networkChanged();
		// The origin sees the close land on its own turn.
		await settlePool();

		t.ok(
			tracker.stats().connections[0].closedAt !== null,
			"the pooled connection is closed rather than left open until it times out",
		);

		const r3 = await fetch(tracker.url("/get"), { agent });
		await r3.text();

		t.equal(
			tracker.stats().totalConnections,
			2,
			"and the next request connects afresh rather than reusing",
		);
	} finally {
		await tracker.close();
	}
});

test("networkChanged leaves in-flight requests alone", async (t) => {
	t.plan(3);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		// In flight when the signal lands: the response is still being waited on.
		const inFlight = fetch(tracker.url("/delay/300"), { agent });
		await new Promise((resolve) => setTimeout(resolve, 50));

		agent.networkChanged();

		const response = await inFlight;
		t.ok(response.ok, "the in-flight request completes rather than failing");
		const body = await response.json();
		t.equal(body.delayed, 300, "and delivers the response it was waiting for");

		// The agent is still usable afterwards.
		const after = await fetch(tracker.url("/get"), { agent });
		await after.text();
		t.ok(after.ok, "and the agent still serves requests after the signal");
	} finally {
		await tracker.close();
	}
});

test("networkChanged keeps working through a streaming body", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		// Headers have arrived, the body has not: the signal must not cut it off.
		const response = await fetch(tracker.url("/stream/5/40"), { agent });
		agent.networkChanged();

		const body = await response.bytes();
		t.equal(body.length, 500, "a body still downloading is not interrupted");
		t.ok(
			tracker.stats().totalRequests >= 1,
			"the request was served by the origin",
		);
	} finally {
		await tracker.close();
	}
});

test("networkChanged does not interrupt a body being written to file", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();
	const dest = path.join(
		os.tmpdir(),
		`faith-netchange-tofile-${Date.now()}.bin`,
	);

	try {
		const agent = new Agent();

		// toFile() streams the body straight to disk, so the signal lands part way
		// through a write rather than part way through a read.
		const response = await fetch(tracker.url("/stream/5/40"), { agent });
		const writing = response.toFile(dest);
		agent.networkChanged();

		const result = await writing;
		t.equal(result.bytesWritten, 500, "the whole body reaches the file");
		t.ok(
			tracker.stats().totalRequests >= 1,
			"having been served by the origin",
		);
	} finally {
		await tracker.close();
	}
});

test("networkChanged re-resolves names after flushing the DNS cache", async (t) => {
	t.plan(2);

	const agent = new Agent();

	await agent.prefetchDns(hostname().split(":")[0]);
	agent.networkChanged();

	// The cache is empty, so this request pays for a lookup again; what matters to
	// the caller is that it still resolves and succeeds.
	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "a request after the flush resolves and succeeds");
	await response.text();

	agent.networkChanged();
	const again = await fetch(url("/get"), { agent });
	t.ok(again.ok, "and again after a second signal");
	await again.text();
});

test("networkChanged flushes nothing under the system resolver", async (t) => {
	t.plan(1);

	// There is no in-process DNS cache to flush with dns.system, so the signal has
	// no DNS work to do and must not fail for the want of it.
	const agent = new Agent({ dns: { system: true } });

	const first = await fetch(url("/get"), { agent });
	await first.text();

	agent.networkChanged();

	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "requests keep working after the signal");
	await response.text();
});

test("networkChanged clears the record of warmed origins", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();
		const origin = new URL(tracker.url("/")).origin;

		await agent.preconnect(origin);
		t.equal(
			tracker.stats().totalConnections,
			1,
			"the warm-up opens a connection",
		);

		await settlePool();
		agent.networkChanged();

		// Without the record being cleared this second warm-up would see the origin
		// as already warm and do nothing.
		await agent.preconnect(origin);
		t.equal(
			tracker.stats().totalConnections,
			2,
			"after the signal the origin is no longer warm, so warming it works again",
		);
	} finally {
		await tracker.close();
	}
});

test("networkChanged keeps connections() listing an in-flight connection", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const first = await fetch(tracker.url("/get"), { agent });
		await first.text();

		const listedBefore = agent.connections();
		t.ok(listedBefore.length >= 1, "the connection is listed before the signal");

		// The signal cannot tell a dropped idle connection from one still carrying a
		// request, so it clears neither: entries lapse on the pool idle window.
		agent.networkChanged();

		t.equal(
			agent.connections().length,
			listedBefore.length,
			"the listing is a report rather than state the agent decides from, so it stands",
		);
	} finally {
		await tracker.close();
	}
});

test("networkChanged keeps the cookie jar", async (t) => {
	t.plan(2);

	const agent = new Agent({ cookies: true });

	agent.addCookie(url("/"), "session=kept");
	t.equal(agent.getCookie(url("/")), "session=kept", "the cookie is set");

	agent.networkChanged();

	t.equal(
		agent.getCookie(url("/")),
		"session=kept",
		"a cookie is not a claim about a network path, so the signal keeps it",
	);
});

test("networkChanged keeps the stats counters", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent });
	await response.text();

	const before = agent.stats();
	agent.networkChanged();

	// Compared whole rather than counter by counter, so a counter added later is
	// covered too: the rebuild must not reset any of them.
	t.deepEqual(
		agent.stats(),
		before,
		"the counters record what the agent has done, which the signal does not edit",
	);
	t.ok(before.requestsSent >= 1, "and there was something to preserve");
});

test("networkChanged keeps agent configuration in force", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "x-faith-test", value: "kept" }],
		userAgent: "NetworkChangeTest/1.0",
	});

	agent.networkChanged();

	const response = await fetch(url("/get"), { agent });
	const body = await response.json();
	const headers = body.headers;

	t.deepEqual(
		headers["X-Faith-Test"],
		["kept"],
		"default headers survive the client being rebuilt",
	);
	t.deepEqual(
		headers["User-Agent"],
		["NetworkChangeTest/1.0"],
		"as does the user agent",
	);
});

test("networkChanged keeps the in-memory HTTP cache", async (t) => {
	t.plan(3);

	const agent = new Agent({
		cache: { store: "memory", mode: "force-cache" },
	});

	const first = await fetch(url("/cache/60"), { agent });
	t.ok(first.ok, "the first request populates the cache");
	await first.text();

	agent.networkChanged();

	const second = await fetch(url("/cache/60"), { agent });
	t.ok(second.ok, "the second request succeeds after the signal");
	t.equal(
		second.headers.get("x-cache"),
		"HIT",
		"and is served from the cache the signal kept, not refetched",
	);
	await second.text();
});

test("networkChanged is idempotent and safe with nothing to reset", async (t) => {
	t.plan(2);

	const agent = new Agent();

	// Nothing has been done with this agent yet: no pool, no lookups, no knowledge.
	agent.networkChanged();
	agent.networkChanged();
	agent.networkChanged();
	t.pass("repeated signals on an untouched agent are harmless");

	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "and the agent works normally afterwards");
	await response.text();
});

test("networkChanged on a closed agent does nothing", async (t) => {
	t.plan(2);

	const agent = new Agent();
	const response = await fetch(url("/get"), { agent });
	await response.text();

	agent.close();

	// A closed agent has already released everything the signal would clear, so
	// unlike preconnect/prefetchDns this does not throw (spec:NETCHG#availability).
	agent.networkChanged();
	t.pass("the signal is a no-op rather than an error");

	try {
		await fetch(url("/get"), { agent });
		t.fail("a closed agent should still refuse requests");
	} catch (error) {
		t.equal(error.code, "Closed", "and the agent is still closed afterwards");
	}
});

test("networkChanged keeps a disk HTTP cache", async (t) => {
	t.plan(2);

	const cachePath = path.join(
		os.tmpdir(),
		`faith-netchange-cache-${Date.now()}`,
	);
	const agent = new Agent({
		cache: { store: "disk", path: cachePath, mode: "force-cache" },
	});
	const target = url("/cache/60");

	const first = await fetch(target, { agent });
	t.ok(first.ok, "the first request populates the cache");
	await first.text();

	agent.networkChanged();

	const second = await fetch(target, { agent });
	t.equal(
		second.headers.get("x-cache"),
		"HIT",
		"the rebuilt client reads the same cache directory",
	);
	await second.text();
});

test("networkChanged keeps the redirect policy", async (t) => {
	t.plan(1);

	const agent = new Agent({ redirect: "error" });
	agent.networkChanged();

	try {
		await fetch(url("/redirect/1"), { agent });
		t.fail("a redirect should have been refused");
	} catch (error) {
		t.equal(
			error.code,
			"Redirect",
			"the redirect policy survives the client being rebuilt",
		);
	}
});

test("networkChanged keeps the timeout options", async (t) => {
	t.plan(1);

	const agent = new Agent({ timeout: { total: 150 } });
	agent.networkChanged();

	try {
		await fetch(url("/delay/2"), { agent });
		t.fail("the request should have timed out");
	} catch (error) {
		t.equal(
			error.code,
			"Timeout",
			"the timeout still bounds requests after the signal",
		);
	}
});

test("networkChanged keeps localAddress bound", async (t) => {
	t.plan(1);

	const agent = new Agent({ localAddress: "127.0.0.1" });
	agent.networkChanged();

	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "the source address still binds after the signal");
	await response.text();
});

test("networkChanged stops an in-flight warm-up marking its origin warm", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();
		const origin = new URL(tracker.url("/")).origin;

		// The signal lands while the warm-up is still in flight: its connection goes
		// into the pool that is being dropped, so the origin must not end up warm.
		const warming = agent.preconnect(origin);
		agent.networkChanged();
		await warming;

		const opened = tracker.stats().totalConnections;
		await agent.preconnect(origin);

		t.ok(
			tracker.stats().totalConnections > opened,
			"the next warm-up opens a connection rather than finding the origin warm",
		);

		const response = await fetch(tracker.url("/get"), { agent });
		t.ok(response.ok, "and requests to the origin still work");
		await response.text();
	} finally {
		await tracker.close();
	}
});

// An HTTP/2-over-TLS origin reporting the flow-control windows the client
// advertised, so a rebuilt client can be checked against what actually reaches
// the wire rather than against the options it was given. Same shape as the one in
// flow-control.test.js.
async function tlsOrigin() {
	const { ca, certPath, keyPath } = ensureCert();
	const sockets = new Set();
	const server = http2.createSecureServer({
		key: readFileSync(keyPath),
		cert: readFileSync(certPath),
	});
	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
	server.on("stream", (stream) => {
		const observed = {
			streamWindow: stream.session.remoteSettings.initialWindowSize,
			connectionWindow: stream.session.state.remoteWindowSize,
		};
		stream.respond({ ":status": 200, "content-type": "application/json" });
		stream.end(JSON.stringify(observed));
	});

	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});

	return {
		url: `https://127.0.0.1:${server.address().port}`,
		ca,
		close: () =>
			new Promise((resolve) => {
				for (const socket of sockets) socket.destroy();
				sockets.clear();
				server.close(resolve);
				setTimeout(resolve, 500).unref();
			}),
	};
}

test("networkChanged keeps TLS trust and flow-control windows", async (t) => {
	const server = await tlsOrigin();
	const agent = new Agent({
		tls: { extraRoots: [server.ca] },
		flowControl: { streamWindow: 3 * 1024 * 1024 },
	});

	try {
		const before = await fetch(server.url, { agent, timeout: 10000 });
		t.equal(before.version, "HTTP/2.0", "negotiated h2 over the test CA");
		t.equal(
			(await before.json()).streamWindow,
			3 * 1024 * 1024,
			"with the configured stream window",
		);

		agent.networkChanged();

		const after = await fetch(server.url, { agent, timeout: 10000 });
		t.ok(
			after.ok,
			"the rebuilt client still trusts the CA from tls.extraRoots",
		);
		t.equal(
			(await after.json()).streamWindow,
			3 * 1024 * 1024,
			"and still advertises the configured stream window",
		);
	} finally {
		agent.close();
		await server.close();
		t.end();
	}
});

test("networkChanged keeps http3 hints confirmed", async (t) => {
	t.plan(1);

	// A hint is the caller's assertion that an origin speaks HTTP/3, so it must
	// survive the signal or an HTTP/3-only origin becomes unreachable. Nothing is
	// fetched here: the assertion is that the agent accepts the signal with hints
	// seeded and stays usable, with the state-level behaviour covered by the Rust
	// unit tests in src/alt_svc.rs.
	const agent = new Agent({
		http3: { hints: [{ host: "example.com", port: 443 }] },
	});

	agent.networkChanged();

	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "the agent keeps working with hinted origins in place");
	await response.text();
});
