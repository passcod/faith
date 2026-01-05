const test = require("tape");
const { Agent, fetch } = require("../wrapper.js");

const HTTPBIN_URL = process.env.HTTPBIN_URL || "http://localhost:8888";

test("connections returns empty array initially", async (t) => {
	const agent = new Agent();
	const stats = agent.connections();
	t.ok(Array.isArray(stats), "returns an array");
	t.equal(stats.length, 0, "array is empty before any requests");
	t.end();
});

test("connections tracks connections after requests", async (t) => {
	const agent = new Agent();

	await fetch(`${HTTPBIN_URL}/get`, { agent });

	const stats = agent.connections();
	t.ok(Array.isArray(stats), "returns an array");
	t.ok(stats.length > 0, "has at least one tracked connection");

	const conn = stats[0];
	t.equal(conn.connectionType, "tcp", "connectionType is tcp");
	t.ok(conn.localAddress, "has local address");
	t.ok(typeof conn.localPort === "number", "has local port");
	t.ok(conn.remoteAddress, "has remote address");
	t.ok(typeof conn.remotePort === "number", "has remote port");
	t.ok(conn.firstSeen instanceof Date, "has firstSeen timestamp");
	t.ok(conn.lastSeen instanceof Date, "has lastSeen timestamp");
	t.ok(conn.lastSeen >= conn.firstSeen, "lastSeen >= firstSeen");
	t.equal(conn.responseCount, 1, "responseCount is 1 after one request");

	t.end();
});

test("connections includes TCP info when available", async (t) => {
	const agent = new Agent();

	// Use a slow request to ensure connection stays open
	const promise = fetch(
		`${HTTPBIN_URL}/drip?duration=1&numbytes=100&delay=0`,
		{ agent },
	);

	// Wait a bit then query stats
	await new Promise((r) => setTimeout(r, 300));

	const stats = agent.connections();
	t.ok(stats.length > 0, "has tracked connections");

	const conn = stats[0];
	if (conn.rttUs !== undefined) {
		t.ok(typeof conn.rttUs === "number", "rttUs is a number");
		t.ok(conn.rttUs >= 0, "rttUs is non-negative");
	}
	if (conn.congestionWindow !== undefined) {
		t.ok(
			typeof conn.congestionWindow === "number",
			"congestionWindow is a number",
		);
		t.ok(conn.congestionWindow > 0, "congestionWindow is positive");
	}

	// Finish the request
	const resp = await promise;
	await resp.text();

	t.end();
});

test("responseCount increments with each request on same connection", async (t) => {
	const agent = new Agent();

	// Make multiple requests that should reuse the connection
	for (let i = 0; i < 3; i++) {
		const resp = await fetch(`${HTTPBIN_URL}/get`, { agent });
		await resp.text();
	}

	const stats = agent.connections();
	t.ok(stats.length > 0, "has tracked connections");
	console.log(stats);

	const totalResponses = stats.reduce(
		(sum, conn) => sum + conn.responseCount,
		0,
	);
	t.equal(
		totalResponses,
		3,
		"total responseCount across all connections is 3",
	);

	t.end();
});

test("connections tracks multiple connections", async (t) => {
	const agent = new Agent();

	await Promise.all([
		fetch(`${HTTPBIN_URL}/get`, { agent }),
		fetch(`${HTTPBIN_URL}/headers`, { agent }),
		fetch(`${HTTPBIN_URL}/ip`, { agent }),
	]);

	const stats = agent.connections();
	t.ok(Array.isArray(stats), "returns an array");
	t.ok(stats.length >= 1, "has at least one connection (may be pooled)");

	// All connections should be tcp type
	for (const conn of stats) {
		t.equal(conn.connectionType, "tcp", "all connections are tcp type");
	}

	t.end();
});
