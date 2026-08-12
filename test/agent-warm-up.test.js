const test = require("tape");
const { Agent, fetch } = require("../wrapper.js");
const { url, hostname, port } = require("./helpers.js");

const origin = () => `http://${hostname()}`;

test("preconnect leaves a pooled connection the next request rides", async (t) => {
	const agent = new Agent();

	await agent.preconnect(origin());

	const warmed = agent.connections();
	t.equal(warmed.length, 1, "the warm-up connection is listed");
	t.equal(
		warmed[0].responseCount,
		0,
		"listed before any request has used it, at a response count of zero",
	);

	const response = await fetch(url("/get"), { agent });
	await response.text();

	const after = agent.connections();
	t.equal(
		after.length,
		1,
		"the request landed on the warm connection rather than dialling another",
	);
	t.equal(after[0].responseCount, 1, "and is credited with the response");

	t.end();
});

test("a request on a preconnected origin reports reused", async (t) => {
	const agent = new Agent();

	await agent.preconnect(origin());
	const response = await fetch(url("/get"), { agent });
	await response.text();
	const timing = await response.timing;

	// A reused connection does no setup, so the connect phase is empty. This is how a
	// caller confirms the warm-up was spent rather than having lapsed.
	t.equal(
		timing.connectEnd,
		timing.connectStart,
		"no connection setup was paid for",
	);
	t.equal(timing.domainLookupEnd, timing.domainLookupStart, "nor any lookup");

	t.end();
});

test("neither warm-up moves the agent's request accounting", async (t) => {
	const agent = new Agent();

	await agent.preconnect(origin());
	await agent.prefetchDns(hostname().split(":")[0]);

	const stats = agent.stats();
	t.equal(stats.requestsSent, 0, "a warm-up's own request is not counted");
	t.equal(stats.responsesReceived, 0, "nor its response");
	t.equal(stats.bodiesStarted, 0, "nor any body");
	t.equal(stats.bodiesFinished, 0, "nor its completion");

	t.end();
});

test("preconnect reduces a longer URL to its origin", async (t) => {
	const agent = new Agent();

	await agent.preconnect(url("/get?query=1#frag"));

	const conns = agent.connections();
	t.equal(conns.length, 1, "a full URL warms the origin it belongs to");
	t.equal(
		conns[0].remotePort,
		Number(port()),
		"connecting to the origin's port",
	);

	t.end();
});

test("preconnect coalesces with work already done or in progress", async (t) => {
	const agent = new Agent();

	await Promise.all([
		agent.preconnect(origin()),
		agent.preconnect(origin()),
		agent.preconnect(origin()),
	]);
	t.equal(
		agent.connections().length,
		1,
		"concurrent warm-ups are single-flighted rather than opening duplicates",
	);

	await agent.preconnect(origin());
	t.equal(
		agent.connections().length,
		1,
		"an origin that already holds an idle pooled connection does no new work",
	);

	t.end();
});

test("preconnect treats URL variants of one origin as the same origin", async (t) => {
	const agent = new Agent();

	await agent.preconnect(origin());
	t.equal(agent.connections().length, 1, "the origin is warmed");

	// Path, query, fragment and userinfo are stripped, so each of these reduces to the
	// origin already warmed and does no new work.
	for (const variant of [
		url("/get"),
		url("/status/200?x=1"),
		`${origin()}/#frag`,
		`http://user:pass@${hostname()}/`,
	]) {
		await agent.preconnect(variant);
		t.equal(
			agent.connections().length,
			1,
			`${variant} reduces to the origin already warm`,
		);
	}

	t.end();
});

test("a warm-up never rejects, whatever happens on the network", async (t) => {
	const agent = new Agent();

	// Port 1 is reserved and nothing listens there: the connection is refused.
	await agent.preconnect("http://127.0.0.1:1");
	t.pass("a refused connection resolves quietly");

	await agent.prefetchDns("nonexistent-host.invalid");
	t.pass("a DNS failure resolves quietly");

	t.end();
});

test("a warm-up resolves quietly when the agent closes under it", async (t) => {
	const agent = new Agent();

	const warming = agent.preconnect(origin());
	agent.close();
	await warming;
	t.pass("an agent closed mid-flight does not reject the warm-up");

	t.end();
});

test("caller mistakes throw synchronously rather than resolving", async (t) => {
	const agent = new Agent();

	t.throws(
		() => agent.preconnect("not an origin"),
		/invalid IP address/,
		"a malformed origin throws a parse error",
	);
	t.throws(
		() => agent.prefetchDns(""),
		/invalid IP address/,
		"so does a malformed host",
	);

	try {
		agent.preconnect("not an origin");
	} catch (err) {
		t.equal(err.code, "AddressParse", "reported as a parse error");
		t.ok(err instanceof SyntaxError, "of the same class dns.overrides uses");
	}

	t.end();
});

test("a warm-up on a closed agent throws the closed-agent error", async (t) => {
	const agent = new Agent();
	agent.close();

	for (const [method, arg] of [
		["preconnect", origin()],
		["prefetchDns", "localhost"],
	]) {
		try {
			agent[method](arg);
			t.fail(`${method} should throw on a closed agent`);
		} catch (err) {
			t.equal(err.code, "Closed", `${method} reports the closed agent`);
			t.ok(err instanceof TypeError, `${method} throws a TypeError`);
		}
	}

	t.end();
});

test("prefetchDns ignores the parts a DNS name does not have", async (t) => {
	const agent = new Agent();

	// A scheme, port and path in a fuller string are ignored rather than rejected.
	await agent.prefetchDns(url("/get"));
	t.pass("a full URL is reduced to its host");

	await agent.prefetchDns("localhost");
	t.pass("a bare host resolves");

	t.end();
});

test("prefetchDns warms the cache a later request reads", async (t) => {
	const agent = new Agent();
	const host = hostname().split(":")[0];

	await agent.prefetchDns(host);

	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "the request succeeds against the warmed name");
	await response.text();

	t.end();
});

test("prefetchDns resolves without work under the system resolver", async (t) => {
	// There is no in-process cache to warm with getaddrinfo, so the call is a no-op that
	// still resolves successfully.
	const agent = new Agent({ dns: { system: true } });

	await agent.prefetchDns("localhost");
	t.pass("the call resolves");

	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "and requests still work");
	await response.text();

	t.end();
});

test("preconnect honours dns.overrides", async (t) => {
	const agent = new Agent({
		dns: {
			overrides: [
				{ domain: "warmup.tld", addresses: [`127.0.0.1:${port()}`] },
			],
		},
	});

	await agent.preconnect(`http://warmup.tld:${port()}`);
	t.equal(
		agent.connections().length,
		1,
		"the override resolves the warm-up's host",
	);

	const response = await fetch(`http://warmup.tld:${port()}/get`, { agent });
	t.ok(response.ok, "and the request rides the connection it opened");
	await response.text();
	t.equal(
		agent.connections()[0].responseCount,
		1,
		"on the same connection",
	);

	t.end();
});

test("prefetchDns honours dns.overrides", async (t) => {
	const agent = new Agent({
		dns: {
			overrides: [
				{ domain: "prefetch.tld", addresses: [`127.0.0.1:${port()}`] },
			],
		},
	});

	await agent.prefetchDns("prefetch.tld");
	const response = await fetch(`http://prefetch.tld:${port()}/get`, { agent });
	t.ok(response.ok, "the overridden name resolves for the request");
	await response.text();

	t.end();
});

test("the warm-up carries the agent's configuration", async (t) => {
	const agent = new Agent({
		userAgent: "WarmUpAgent/1.0",
		headers: [{ name: "X-Warm", value: "yes" }],
	});

	// The origin sees the synthetic HEAD, so the headers it carries are observable on a
	// request that reports them back.
	await agent.preconnect(origin());

	const response = await fetch(url("/headers"), { agent });
	const data = await response.json();
	const ua = Array.isArray(data.headers["User-Agent"])
		? data.headers["User-Agent"][0]
		: data.headers["User-Agent"];
	t.equal(ua, "WarmUpAgent/1.0", "the agent's userAgent applies");

	t.end();
});

test("preconnect on an https origin negotiates TLS ahead of the request", async (t) => {
	const agent = new Agent();

	// Nothing listens for TLS on the test server, so this exercises the https path's
	// failure rather than its success: it must still resolve quietly.
	await agent.preconnect(`https://${hostname()}`);
	t.pass("an https warm-up that cannot complete resolves quietly");

	t.end();
});

test("a warm-up to one origin does not displace another", async (t) => {
	const agent = new Agent({ pool: { maxIdlePerHost: 1 } });

	await agent.preconnect(origin());
	await agent.preconnect(`http://127.0.0.1:${port()}`);

	t.equal(
		agent.connections().length,
		2,
		"the per-origin cap does not make origins compete",
	);

	t.end();
});

/**
 * An origin that answers the warm-up's `HEAD /` and then abandons the connection, so the
 * connection the pool holds is already gone when the next request claims it.
 *
 * This is the scenario the spec's "Warming an origin that closes idle connections" section
 * describes: a warm-up creates a pooled connection that would not otherwise exist, so it meets
 * the pool's existing dead-connection risk more often. The conformance harness cannot host this
 * — its dropping route is a path of its own, while a warm-up always targets the root — so the
 * origin is built here.
 */
function dropAfterWarmUp() {
	const http = require("node:http");
	const state = { warmUps: 0, dropped: 0 };
	const server = http.createServer((req, res) => {
		req.resume();
		const socket = req.socket;
		const body = "ok";
		res.setHeader("content-type", "text/plain");
		res.setHeader("content-length", String(Buffer.byteLength(body)));

		// Only the warm-up's own request is followed by a close: a request that lands on a
		// fresh connection afterwards must be answered normally, or the test could not tell
		// recovery from the origin simply being broken.
		const isWarmUp = req.method === "HEAD" && req.url === "/";
		if (isWarmUp) state.warmUps++;

		res.end(isWarmUp ? undefined : body, () => {
			if (!isWarmUp) return;
			state.dropped++;
			// Graceful in both directions, for the reasons the idle-close dimension documents:
			// flush, then FIN, then destroy once flushed.
			socket.end();
			socket.once("finish", () => socket.destroy());
		});
	});

	return new Promise((resolve) => {
		server.listen(0, "127.0.0.1", () => {
			const { port } = server.address();
			resolve({
				state,
				origin: `http://127.0.0.1:${port}`,
				close: () => new Promise((done) => server.close(done)),
			});
		});
	});
}

test("a GET recovers when the origin abandoned the warm-up's connection", async (t) => {
	const server = await dropAfterWarmUp();
	const agent = new Agent();

	try {
		await agent.preconnect(server.origin);
		t.equal(server.state.warmUps, 1, "the origin saw the warm-up's HEAD /");
		t.equal(server.state.dropped, 1, "and abandoned the connection under it");

		// A GET is replayable, so the dead warm-up connection must never reach the caller:
		// this is what keeps a warm-up invisible in the usual case.
		const response = await fetch(`${server.origin}/after`, { agent, timeout: 10_000 });
		t.equal(response.status, 200, "the GET was sent again on another connection");
		t.equal(await response.text(), "ok", "and came back with the right answer");
	} finally {
		agent.close();
		await server.close();
		t.end();
	}
});

test("a POST is not replayed onto a dead warm-up connection", async (t) => {
	const server = await dropAfterWarmUp();
	const agent = new Agent();

	try {
		await agent.preconnect(server.origin);

		// A POST is deliberately not sent again, because nothing in the error says whether the
		// origin processed it. So it may surface as a failure — the trade the spec states — but
		// it must never come back with the wrong answer.
		let outcome;
		try {
			const response = await fetch(`${server.origin}/after`, {
				agent,
				method: "POST",
				body: "payload",
				timeout: 10_000,
			});
			outcome = { status: response.status, body: await response.text() };
		} catch (err) {
			outcome = { failed: err.code || err.message };
		}

		if (outcome.failed) {
			t.pass(`the POST surfaced the dead connection to the caller (${outcome.failed})`);
		} else {
			t.equal(outcome.status, 200, "or it reached a live connection and was answered");
			t.equal(outcome.body, "ok", "with the right answer either way");
		}
	} finally {
		agent.close();
		await server.close();
		t.end();
	}
});

test("preconnect does no new work for an origin ordinary traffic already warmed", async (t) => {
	const agent = new Agent();

	// The criterion is about the origin holding an idle pooled connection, not about how it came
	// to hold one, so a plain request satisfies it just as a warm-up does.
	const response = await fetch(url("/get"), { agent });
	await response.text();
	t.equal(agent.connections().length, 1, "the request pooled a connection");
	t.equal(agent.connections()[0].responseCount, 1, "credited to the request");

	await agent.preconnect(origin());
	const after = agent.connections();
	t.equal(after.length, 1, "the warm-up opened no second connection");
	t.equal(
		after[0].responseCount,
		1,
		"and sent no redundant HEAD, so the count is untouched",
	);

	t.end();
});
