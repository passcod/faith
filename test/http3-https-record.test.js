/**
 * Reading an `HTTPS` DNS record (RFC 9460) as an HTTP/3 advertisement.
 *
 * An `Alt-Svc` header can only arrive on a response, so discovering HTTP/3 that
 * way costs a TCP round trip first. An `HTTPS` record is available while the
 * name is being resolved, before anything has connected, so an origin
 * advertising `alpn="h3"` can be probe-worthy from its very first request.
 *
 * These tests pin the resolver half of that: the query is made alongside the
 * address lookups, it does not disturb the request that triggered it, and it is
 * not made at all when there is nothing that could act on the answer. The
 * upgrade half (an advertisement becoming a confirmed origin) needs a working
 * QUIC path and lives in http3-probe.test.js.
 */

const test = require("tape");
const { Agent } = require("../index.js");
const { fetch } = require("../wrapper.js");
const { startDnsServer } = require("./lib/dns-server.js");

const HTTPBIN = process.env.HTTPBIN_URL || "http://localhost:8888";
const HTTPBIN_PORT = new URL(HTTPBIN).port || "80";

/** Give the spawned HTTPS query a moment to land; it deliberately races nothing. */
async function settle(ms = 500) {
	await new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * An agent resolving through `server` only, with the exemptions that would
 * otherwise send a `.test` name to the system resolver lifted by using a name
 * the zone answers for.
 */
function agentFor(server, options = {}) {
	return new Agent({
		dns: { servers: [`udp://${server.host}:${server.port}`], timeout: 2000 },
		...options,
	});
}

test("an HTTPS record is queried alongside the address lookup", async (t) => {
	const server = await startDnsServer({
		zone: {
			"h3origin.test": {
				a: ["127.0.0.1"],
				https: [{ priority: 1, alpn: ["h2", "h3"] }],
				ttl: 30,
			},
		},
	});
	const agent = agentFor(server);

	try {
		const response = await fetch(
			`http://h3origin.test:${HTTPBIN_PORT}/get`,
			{ agent },
		);
		t.equal(response.status, 200, "the request the lookup was for succeeds");
		await response.text();

		await settle();

		t.ok(
			server.countFor("h3origin.test", "A") > 0,
			"the address lookup happened",
		);
		t.ok(
			server.countFor("h3origin.test", "HTTPS") > 0,
			"and the HTTPS record was asked for as well",
		);
	} finally {
		await agent.close();
		await server.close();
	}
});

test("a name with no HTTPS record resolves and fetches unaffected", async (t) => {
	// The overwhelmingly common case: the negative answer must be a non-event,
	// not something that shows up as a failed or delayed request.
	const server = await startDnsServer({
		zone: { "plain.test": { a: ["127.0.0.1"], ttl: 30 } },
	});
	const agent = agentFor(server);

	try {
		const response = await fetch(`http://plain.test:${HTTPBIN_PORT}/get`, {
			agent,
		});
		t.equal(response.status, 200, "the request succeeds");
		await response.text();

		await settle();

		t.ok(
			server.countFor("plain.test", "HTTPS") > 0,
			"the query was still made",
		);
		t.pass("and answering it with nothing changed nothing about the request");
	} finally {
		await agent.close();
		await server.close();
	}
});

test("no HTTPS record is queried with HTTP/3 upgrade disabled", async (t) => {
	// Nothing could act on the answer, so asking for it would be pure cost
	// (spec:DNS#https-records).
	const server = await startDnsServer({
		zone: {
			"noupgrade.test": {
				a: ["127.0.0.1"],
				https: [{ priority: 1, alpn: ["h3"] }],
				ttl: 30,
			},
		},
	});
	const agent = agentFor(server, { http3: { upgradeEnabled: false } });

	try {
		const response = await fetch(
			`http://noupgrade.test:${HTTPBIN_PORT}/get`,
			{ agent },
		);
		t.equal(response.status, 200, "the request still succeeds");
		await response.text();

		await settle();

		t.ok(
			server.countFor("noupgrade.test", "A") > 0,
			"the address lookup still happened",
		);
		t.equal(
			server.countFor("noupgrade.test", "HTTPS"),
			0,
			"but no HTTPS record was asked for",
		);
	} finally {
		await agent.close();
		await server.close();
	}
});

test("a resolver that never answers the HTTPS query does not hold up the request", async (t) => {
	// The query is spawned rather than awaited, so a resolver that swallows it
	// must cost the request nothing (spec:DNS#https-records). The failure is
	// scoped to the HTTPS type so the addresses still answer: this has to be a
	// test about the extra query, not about resolution failing.
	const server = await startDnsServer({
		zone: { "slowsvc.test": { a: ["127.0.0.1"], ttl: 30 } },
	});
	server.fail("slowsvc.test", "drop", { type: "HTTPS" });
	// `dns.timeout` is the deadline a blocking query would burn, so the budget
	// below is well under it: waiting on the dropped query is unmissable.
	const agent = agentFor(server);

	try {
		const started = Date.now();
		const response = await fetch(`http://slowsvc.test:${HTTPBIN_PORT}/get`, {
			agent,
		});
		const elapsed = Date.now() - started;
		t.equal(response.status, 200, "the request succeeds on the first attempt");
		await response.text();

		t.ok(
			elapsed < 1500,
			`it did not wait on the dropped HTTPS query (took ${elapsed}ms)`,
		);

		await settle();

		t.ok(
			server.countFor("slowsvc.test", "HTTPS") > 0,
			"and the query really was made and really was dropped",
		);
		t.ok(
			server.countFor("slowsvc.test", "A") > 0,
			"while the address lookup answered normally",
		);
	} finally {
		await agent.close();
		await server.close();
	}
});
