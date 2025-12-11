const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Agent with timeout.connect option", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { connect: 5000 },
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with connect timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with timeout.read option", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { read: 5000 },
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with read timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with timeout.total option", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with total timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with all timeout options", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: {
			connect: 5000,
			read: 5000,
			total: 10000,
		},
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with all timeout options");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout.connect with slow endpoint", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { connect: 5000 },
	});

	const response = await faithFetch(url("/delay/1"), { agent });
	t.ok(response.ok, "Should successfully fetch delayed endpoint");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout.total triggers timeout on slow endpoint", async (t) => {
	t.plan(1);

	const agent = new Agent({
		timeout: { total: 100 },
	});

	try {
		await faithFetch(url("/delay/10"), { agent });
		t.fail("Should throw timeout error");
	} catch (err) {
		t.pass("Should throw error for total timeout");
	}
});

test("Agent timeout.total allows fast requests", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 2000 },
	});

	const response = await faithFetch(url("/delay/1"), { agent });
	t.ok(response.ok, "Should successfully fetch within total timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout persists across multiple requests", async (t) => {
	t.plan(4);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	const response1 = await faithFetch(url("/get"), { agent });
	t.ok(response1.ok, "First request should succeed");

	const response2 = await faithFetch(url("/headers"), { agent });
	t.ok(response2.ok, "Second request should succeed");

	try {
		await faithFetch(url("/delay/10"), { agent });
		t.fail("Should throw timeout error");
	} catch (err) {
		t.pass("Should throw timeout error on slow request");
	}

	const response3 = await faithFetch(url("/get"), { agent });
	t.ok(response3.ok, "Should continue working after timeout");
});

test("Agent timeout with POST request", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: JSON.stringify({ test: "data" }),
		headers: { "Content-Type": "application/json" },
		agent,
	});

	t.ok(response.ok, "Should successfully POST with timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout with streaming response", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { read: 5000, total: 10000 },
	});

	const response = await faithFetch(url("/stream/20"), { agent });
	t.ok(response.ok, "Should successfully fetch streaming endpoint");

	const text = await response.text();
	t.ok(text.length > 0, "Should receive streamed data");
});

test("Different agents can have different timeouts", async (t) => {
	t.plan(3);

	const agent1 = new Agent({
		timeout: { total: 5000 },
	});

	const agent2 = new Agent({
		timeout: { total: 100 },
	});

	const response1 = await faithFetch(url("/delay/1"), { agent: agent1 });
	t.ok(response1.ok, "Agent1 should succeed with longer timeout");

	try {
		await faithFetch(url("/delay/10"), { agent: agent2 });
		t.fail("Agent2 should timeout");
	} catch (err) {
		t.pass("Agent2 should throw timeout error");
	}

	const response2 = await faithFetch(url("/get"), { agent: agent2 });
	t.ok(response2.ok, "Agent2 should work for fast requests");
});

test("Agent timeout with parallel requests", async (t) => {
	t.plan(3);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	const promises = [
		faithFetch(url("/get"), { agent }),
		faithFetch(url("/headers"), { agent }),
		faithFetch(url("/status/200"), { agent }),
	];

	const responses = await Promise.all(promises);

	t.ok(responses[0].ok, "First parallel request should succeed");
	t.ok(responses[1].ok, "Second parallel request should succeed");
	t.ok(responses[2].ok, "Third parallel request should succeed");
});

test("Agent timeout with other agent options", async (t) => {
	t.plan(4);

	const agent = new Agent({
		timeout: { total: 5000 },
		userAgent: "TimeoutAgent/1.0",
		headers: [{ name: "X-Custom", value: "test" }],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	const userAgent = Array.isArray(data.headers["User-Agent"])
		? data.headers["User-Agent"][0]
		: data.headers["User-Agent"];
	const customHeader = Array.isArray(data.headers["X-Custom"])
		? data.headers["X-Custom"][0]
		: data.headers["X-Custom"];
	t.equal(userAgent, "TimeoutAgent/1.0", "User-Agent set");
	t.equal(customHeader, "test", "Custom header set");
	t.ok(true, "All options work together");
});

test("Agent timeout with cookies", async (t) => {
	t.plan(1);

	const agent = new Agent({
		timeout: { total: 5000 },
		cookies: true,
	});

	const cookiesUrl = url("/cookies");
	agent.addCookie(cookiesUrl, "session=test");

	const response = await faithFetch(cookiesUrl, { agent });
	t.ok(response.ok, "Should successfully fetch with timeout and cookies");
});

test("Agent with empty timeout object", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: {},
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with empty timeout object");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout applies to redirects", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	const response = await faithFetch(url("/redirect/2"), { agent });
	t.ok(response.ok, "Should successfully handle redirects with timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout with error response", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	const response = await faithFetch(url("/status/404"), { agent });
	t.equal(response.status, 404, "Should receive 404 status");
	t.notOk(response.ok, "Response should not be ok");
});

test("Agent timeout stats tracking", async (t) => {
	t.plan(4);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	await faithFetch(url("/get"), { agent });

	try {
		await faithFetch(url("/delay/10"), { agent });
	} catch (err) {}

	await faithFetch(url("/headers"), { agent });

	const stats = agent.stats();
	t.equal(stats.requestsSent, 3, "Should track all requests");
	t.equal(stats.responsesReceived, 2, "Should track successful responses");
	t.ok(
		stats.responsesReceived < stats.requestsSent,
		"Timeout should not count as response",
	);
	t.equal(
		stats.requestsSent - stats.responsesReceived,
		1,
		"One request timed out",
	);
});

test("Agent timeout.total shorter than delay fails", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 500 },
	});

	try {
		await faithFetch(url("/delay/2"), { agent });
		t.fail("Should throw timeout error");
	} catch (err) {
		t.pass("Should throw error when total timeout exceeded");
		t.ok(err.message, "Error should have message");
	}
});

test("Agent timeout with very short timeout fails fast", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 1 },
	});

	const start = Date.now();
	try {
		await faithFetch(url("/delay/5"), { agent });
		t.fail("Should throw timeout error");
	} catch (err) {
		const elapsed = Date.now() - start;
		t.pass("Should throw timeout error");
		t.ok(elapsed < 2000, "Should fail quickly, not wait for full delay");
	}
});

test("Agent timeout.connect vs timeout.total", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: {
			connect: 1000,
			total: 3000,
		},
	});

	const response = await faithFetch(url("/delay/1"), { agent });
	t.ok(response.ok, "Should succeed when within total timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout.read applies per read operation", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { read: 3000 },
	});

	const response = await faithFetch(url("/drip?duration=2&numbytes=10"), {
		agent,
	});
	t.ok(response.ok, "Should successfully fetch with read timeout");

	const text = await response.text();
	t.ok(text.length > 0, "Should receive data");
});

test("Agent timeout with mixed success and timeout", async (t) => {
	t.plan(5);

	const agent = new Agent({
		timeout: { total: 2000 },
	});

	const response1 = await faithFetch(url("/get"), { agent });
	t.ok(response1.ok, "Fast request should succeed");

	try {
		await faithFetch(url("/delay/10"), { agent });
		t.fail("Slow request should timeout");
	} catch (err) {
		t.pass("Slow request should throw timeout error");
	}

	const response2 = await faithFetch(url("/headers"), { agent });
	t.ok(response2.ok, "Another fast request should succeed");

	try {
		await faithFetch(url("/delay/5"), { agent });
		t.fail("Another slow request should timeout");
	} catch (err) {
		t.pass("Another slow request should throw timeout error");
	}

	const response3 = await faithFetch(url("/get"), { agent });
	t.ok(response3.ok, "Final fast request should succeed");
});

test("Agent without timeout option works normally", async (t) => {
	t.plan(2);

	const agent = new Agent();
	const response = await faithFetch(url("/delay/1"), { agent });

	t.ok(response.ok, "Should work without timeout option");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout does not affect request-level timeout", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 10000 },
	});

	try {
		await faithFetch(url("/delay/5"), { agent, timeout: 100 });
		t.fail("Should throw timeout error");
	} catch (err) {
		t.pass("Request-level timeout should still work");
		t.ok(err.message, "Error should have message");
	}
});

test("Agent timeout.total with large value allows slow requests", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 30000 },
	});

	const response = await faithFetch(url("/delay/2"), { agent });
	t.ok(response.ok, "Should successfully fetch slow endpoint");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout values are independent", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: {
			connect: 1000,
			read: 2000,
			total: 5000,
		},
	});

	const response = await faithFetch(url("/delay/1"), { agent });
	t.ok(response.ok, "Should successfully fetch");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent timeout with POST and large body", async (t) => {
	t.plan(2);

	const agent = new Agent({
		timeout: { total: 5000 },
	});

	const largeBody = JSON.stringify({ data: "x".repeat(10000) });

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: largeBody,
		headers: { "Content-Type": "application/json" },
		agent,
	});

	t.ok(response.ok, "Should successfully POST large body");
	t.equal(response.status, 200, "Status should be 200");
});
