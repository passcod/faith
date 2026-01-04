const test = require("tape");
const { fetch, Agent } = require("../wrapper.js");
const { createConnectionTracker } = require("./fixtures/connection-tracker.js");

test("consuming body with text() allows connection reuse", async (t) => {
	t.plan(3);

	const tracker = createConnectionTracker();
	const port = await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		await r1.text();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const r3 = await fetch(tracker.url("/get"), { agent });
		await r3.text();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 1, "should reuse single connection");
		t.equal(stats.totalRequests, 3, "should have made 3 requests");
		t.equal(
			stats.connections[0].requests.length,
			3,
			"all requests should be on same connection",
		);
	} finally {
		await tracker.close();
	}
});

test("consuming body with json() allows connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	const port = await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		await r1.json();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.json();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 1, "should reuse connection");
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("consuming body with bytes() allows connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/bytes/100"), { agent });
		await r1.bytes();

		const r2 = await fetch(tracker.url("/bytes/100"), { agent });
		await r2.bytes();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 1, "should reuse connection");
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("fully consuming body stream allows connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		const stream1 = r1.body;
		const reader1 = stream1.getReader();
		while (true) {
			const { done } = await reader1.read();
			if (done) break;
		}
		reader1.releaseLock();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(
			stats.totalConnections,
			1,
			"should reuse connection after stream consumed",
		);
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("unconsumed body prevents connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		// Don't consume body

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 2, "should need new connection");
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("accessing body property without consuming prevents connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		r1.body; // Access but don't consume

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 2, "should need new connection");
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("partial stream read prevents connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		// Use streaming endpoint that sends multiple chunks over time
		// /stream/5/30 = 5 chunks with 30ms delay between each
		const r1 = await fetch(tracker.url("/stream/5/30"), { agent });
		const stream = r1.body;
		const reader = stream.getReader();
		await reader.read(); // Read only first chunk
		reader.releaseLock();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(
			stats.totalConnections,
			2,
			"should need new connection for partial read",
		);
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("clone: reading from one clone allows connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		const r1Clone = r1.clone();

		// Only read from one clone - but SharedStream means body is fully consumed
		await r1.text();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 1, "should reuse connection");
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("clone: reading from both clones allows connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		const r1Clone = r1.clone();

		await r1.text();
		await r1Clone.text();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 1, "should reuse connection");
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("204 No Content allows connection reuse without body access", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/status/204"), { agent });
		// No body to consume for 204

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 1, "should reuse connection for 204");
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("sequential requests with proper consumption reuse connection", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		for (let i = 0; i < 5; i++) {
			const r = await fetch(tracker.url("/get"), { agent });
			await r.text();
		}

		const stats = tracker.stats();
		t.equal(
			stats.totalConnections,
			1,
			"should reuse single connection for all requests",
		);
		t.equal(stats.totalRequests, 5, "should have made 5 requests");
	} finally {
		await tracker.close();
	}
});

test("parallel requests use multiple connections then reuse", async (t) => {
	t.plan(3);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		// Make 3 parallel requests - should need 3 connections
		const responses = await Promise.all([
			fetch(tracker.url("/delay/50"), { agent }),
			fetch(tracker.url("/delay/50"), { agent }),
			fetch(tracker.url("/delay/50"), { agent }),
		]);

		await Promise.all(responses.map((r) => r.text()));

		const statsAfterParallel = tracker.stats();
		t.equal(
			statsAfterParallel.totalConnections,
			3,
			"should use 3 connections for parallel requests",
		);

		// Now make sequential requests - should reuse
		const r4 = await fetch(tracker.url("/get"), { agent });
		await r4.text();

		const r5 = await fetch(tracker.url("/get"), { agent });
		await r5.text();

		const statsFinal = tracker.stats();
		t.equal(
			statsFinal.totalConnections,
			3,
			"should reuse existing connections for sequential requests",
		);
		t.equal(
			statsFinal.totalRequests,
			5,
			"should have made 5 total requests",
		);
	} finally {
		await tracker.close();
	}
});

test("different agents use different connections", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent1 = new Agent();
		const agent2 = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent: agent1 });
		await r1.text();

		const r2 = await fetch(tracker.url("/get"), { agent: agent2 });
		await r2.text();

		const stats = tracker.stats();
		t.equal(
			stats.totalConnections,
			2,
			"different agents should use different connections",
		);
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("consuming body after delay still allows connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });

		// Wait a bit before consuming
		await new Promise((resolve) => setTimeout(resolve, 100));
		await r1.text();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(
			stats.totalConnections,
			1,
			"should reuse connection even with delayed consumption",
		);
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("streaming response fully consumed allows connection reuse", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/stream/3/20"), { agent });
		const reader = r1.body.getReader();
		while (true) {
			const { done } = await reader.read();
			if (done) break;
		}
		reader.releaseLock();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const stats = tracker.stats();
		t.equal(
			stats.totalConnections,
			1,
			"should reuse connection after streaming consumed",
		);
		t.equal(stats.totalRequests, 2, "should have made 2 requests");
	} finally {
		await tracker.close();
	}
});

test("mixed consumed and unconsumed affects connection count", async (t) => {
	t.plan(2);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		// Request 1: consume
		const r1 = await fetch(tracker.url("/get"), { agent });
		await r1.text();

		// Request 2: don't consume (holds connection)
		const r2 = await fetch(tracker.url("/get"), { agent });

		// Request 3: needs new connection because r2 holds the first one
		const r3 = await fetch(tracker.url("/get"), { agent });
		await r3.text();

		const stats = tracker.stats();
		t.equal(stats.totalConnections, 2, "should need 2 connections");
		t.equal(stats.totalRequests, 3, "should have made 3 requests");
	} finally {
		await tracker.close();
	}
});

test("body stats align with connection reuse", async (t) => {
	t.plan(4);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		await r1.text();

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const serverStats = tracker.stats();
		const agentStats = agent.stats();

		t.equal(serverStats.totalConnections, 1, "should reuse connection");
		t.equal(agentStats.bodiesStarted, 2, "should have started 2 bodies");
		t.equal(agentStats.bodiesFinished, 2, "should have finished 2 bodies");
		t.equal(
			agentStats.bodiesStarted - agentStats.bodiesFinished,
			0,
			"no bodies in flight",
		);
	} finally {
		await tracker.close();
	}
});

test("unconsumed body: body stats show in-flight, server shows new connection", async (t) => {
	t.plan(4);

	const tracker = createConnectionTracker();
	await tracker.listen();

	try {
		const agent = new Agent();

		const r1 = await fetch(tracker.url("/get"), { agent });
		r1.body; // Access but don't consume

		const agentStatsAfterR1 = agent.stats();
		t.equal(
			agentStatsAfterR1.bodiesStarted - agentStatsAfterR1.bodiesFinished,
			1,
			"one body in flight",
		);

		const r2 = await fetch(tracker.url("/get"), { agent });
		await r2.text();

		const serverStats = tracker.stats();
		const agentStatsFinal = agent.stats();

		t.equal(serverStats.totalConnections, 2, "should need new connection");
		t.equal(
			agentStatsFinal.bodiesStarted,
			2,
			"should have started 2 bodies",
		);
		t.equal(
			agentStatsFinal.bodiesStarted - agentStatsFinal.bodiesFinished,
			1,
			"one body still in flight",
		);
	} finally {
		await tracker.close();
	}
});
