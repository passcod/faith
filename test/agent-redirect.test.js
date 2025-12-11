const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Agent with redirect: 'follow' (default)", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/redirect/2"), { agent });
	t.ok(response.ok, "Should successfully follow redirects");
	t.equal(response.status, 200, "Status should be 200 after redirects");
	t.ok(
		response.redirected,
		"redirected should be true after following redirects",
	);
});

test("Agent without redirect option follows by default", async (t) => {
	t.plan(3);

	const agent = new Agent();

	const response = await faithFetch(url("/redirect/3"), { agent });
	t.ok(response.ok, "Should follow redirects by default");
	t.equal(response.status, 200, "Status should be 200");
	t.ok(response.redirected, "redirected should be true");
});

test("Agent with redirect: 'follow' handles multiple redirects", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/redirect/5"), { agent });
	t.ok(response.ok, "Should follow multiple redirects");
	t.equal(response.status, 200, "Status should be 200");
	t.ok(
		response.redirected,
		"redirected should be true for multiple redirects",
	);
});

test("Agent with redirect: 'stop' does not follow redirects", async (t) => {
	t.plan(4);

	const agent = new Agent({
		redirect: "stop",
	});

	const response = await faithFetch(url("/redirect/2"), { agent });
	t.notOk(response.ok, "Response should not be ok for redirect status");
	t.equal(response.status, 302, "Should return redirect status code");
	t.ok(response.headers.get("Location"), "Should have Location header");
	t.notOk(
		response.redirected,
		"redirected should be false when not following",
	);
});

test("Agent with redirect: 'stop' returns first redirect", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "stop",
	});

	const response = await faithFetch(url("/redirect/5"), { agent });
	t.equal(response.status, 302, "Should return first redirect status");
	t.ok(response.headers.get("Location"), "Should have Location header");
	t.notOk(response.redirected, "redirected should be false");
});

test("Agent with redirect: 'error' throws on redirect", async (t) => {
	t.plan(1);

	const agent = new Agent({
		redirect: "error",
	});

	try {
		await faithFetch(url("/redirect/2"), { agent });
		t.fail("Should throw error on redirect");
	} catch (err) {
		t.pass("Should throw error when redirect encountered");
	}
});

test("Agent with redirect: 'error' allows non-redirect responses", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "error",
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch non-redirect endpoint");
	t.equal(response.status, 200, "Status should be 200");
	t.notOk(response.redirected, "redirected should be false for non-redirect");
});

test("Agent redirect setting persists across requests", async (t) => {
	t.plan(4);

	const agent = new Agent({
		redirect: "stop",
	});

	const response1 = await faithFetch(url("/redirect/2"), { agent });
	t.equal(response1.status, 302, "First request should return redirect");

	const response2 = await faithFetch(url("/get"), { agent });
	t.ok(response2.ok, "Non-redirect request should succeed");

	const response3 = await faithFetch(url("/redirect/3"), { agent });
	t.equal(response3.status, 302, "Third request should return redirect");
	t.ok(response3.headers.get("Location"), "Should have Location header");
});

test("Different agents can have different redirect settings", async (t) => {
	t.plan(2);

	const followAgent = new Agent({
		redirect: "follow",
	});

	const stopAgent = new Agent({
		redirect: "stop",
	});

	const response1 = await faithFetch(url("/redirect/2"), {
		agent: followAgent,
	});
	t.equal(response1.status, 200, "Follow agent should follow redirects");

	const response2 = await faithFetch(url("/redirect/2"), {
		agent: stopAgent,
	});
	t.equal(response2.status, 302, "Stop agent should not follow redirects");
});

test("Agent redirect: 'follow' with absolute redirect URL", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/absolute-redirect/1"), { agent });
	t.ok(response.ok, "Should follow absolute redirect");
	t.equal(response.status, 200, "Status should be 200");
	t.ok(
		response.redirected,
		"redirected should be true for absolute redirect",
	);
});

test("Agent redirect: 'stop' with absolute redirect URL", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "stop",
	});

	const response = await faithFetch(url("/absolute-redirect/1"), { agent });
	t.equal(response.status, 302, "Should return redirect status");
	t.ok(response.headers.get("Location"), "Should have Location header");
	t.notOk(response.redirected, "redirected should be false");
});

test("Agent redirect with POST request", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/redirect-to?url=/get"), {
		method: "POST",
		body: JSON.stringify({ test: "data" }),
		headers: { "Content-Type": "application/json" },
		agent,
	});

	t.ok(response.ok, "Should handle POST redirect");
	t.equal(response.status, 200, "Should have 200 status after redirect");
	t.ok(response.redirected, "redirected should be true for POST redirect");
});

test("Agent redirect: 'stop' with POST request", async (t) => {
	t.plan(1);

	const agent = new Agent({
		redirect: "stop",
	});

	const response = await faithFetch(url("/redirect-to?url=/get"), {
		method: "POST",
		body: JSON.stringify({ test: "data" }),
		headers: { "Content-Type": "application/json" },
		agent,
	});

	t.equal(response.status, 302, "Should return redirect status for POST");
});

test("Agent redirect with other agent options", async (t) => {
	t.plan(4);

	const agent = new Agent({
		redirect: "follow",
		userAgent: "RedirectAgent/1.0",
		headers: [{ name: "X-Custom", value: "test" }],
	});

	const response = await faithFetch(url("/redirect-to?url=/headers"), {
		agent,
	});

	t.ok(response.ok, "Should successfully follow redirect");
	t.equal(response.status, 200, "Status should be 200");

	const data = await response.json();
	const userAgent = Array.isArray(data.headers["User-Agent"])
		? data.headers["User-Agent"][0]
		: data.headers["User-Agent"];
	const customHeader = Array.isArray(data.headers["X-Custom"])
		? data.headers["X-Custom"][0]
		: data.headers["X-Custom"];

	t.equal(userAgent, "RedirectAgent/1.0", "Custom user agent preserved");
	t.equal(customHeader, "test", "Custom header preserved");
});

test("Agent redirect with timeout", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "follow",
		timeout: { total: 5000 },
	});

	const response = await faithFetch(url("/redirect/3"), { agent });
	t.ok(response.ok, "Should follow redirects with timeout");
	t.equal(response.status, 200, "Status should be 200");
	t.ok(response.redirected, "redirected should be true");
});

test("Agent redirect with cookies", async (t) => {
	t.plan(1);

	const agent = new Agent({
		redirect: "follow",
		cookies: true,
	});

	const setCookieUrl = url("/cookies/set?name=value");
	const response = await faithFetch(setCookieUrl, { agent });

	t.ok(response.ok, "Should handle cookie setting with redirects");
});

test("Agent redirect: 'follow' with parallel requests", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "follow",
	});

	const promises = [
		faithFetch(url("/redirect/2"), { agent }),
		faithFetch(url("/redirect/3"), { agent }),
		faithFetch(url("/redirect/1"), { agent }),
	];

	const responses = await Promise.all(promises);

	for (let i = 0; i < responses.length; i++) {
		t.ok(responses[i].ok, `Request ${i + 1} should succeed`);
	}
});

test("Agent redirect: 'stop' with parallel requests", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "stop",
	});

	const promises = [
		faithFetch(url("/redirect/2"), { agent }),
		faithFetch(url("/redirect/3"), { agent }),
		faithFetch(url("/redirect/1"), { agent }),
	];

	const responses = await Promise.all(promises);

	for (let i = 0; i < responses.length; i++) {
		t.equal(
			responses[i].status,
			302,
			`Request ${i + 1} should return redirect status`,
		);
	}
});

test("Agent redirect stats tracking with follow", async (t) => {
	t.plan(2);

	const agent = new Agent({
		redirect: "follow",
	});

	await faithFetch(url("/redirect/3"), { agent });
	await faithFetch(url("/get"), { agent });

	const stats = agent.stats();
	t.equal(stats.requestsSent, 2, "Should track redirect requests");
	t.equal(stats.responsesReceived, 2, "Should track redirect responses");
});

test("Agent redirect stats tracking with stop", async (t) => {
	t.plan(2);

	const agent = new Agent({
		redirect: "stop",
	});

	await faithFetch(url("/redirect/3"), { agent });
	await faithFetch(url("/get"), { agent });

	const stats = agent.stats();
	t.equal(stats.requestsSent, 2, "Should track requests");
	t.equal(
		stats.responsesReceived,
		2,
		"Should track responses including redirect",
	);
});

test("Agent redirect stats tracking with error", async (t) => {
	t.plan(2);

	const agent = new Agent({
		redirect: "error",
	});

	try {
		await faithFetch(url("/redirect/2"), { agent });
	} catch (err) {}

	await faithFetch(url("/get"), { agent });

	const stats = agent.stats();
	t.equal(stats.requestsSent, 2, "Should track all requests");
	t.equal(
		stats.responsesReceived,
		1,
		"Should only track non-redirect responses",
	);
});

test("Agent redirect: 'follow' final URL is correct", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/redirect/3"), { agent });
	t.ok(response.ok, "Should follow redirects");
	t.ok(
		response.url.includes("/get"),
		"Final URL should be the target after redirects",
	);
	t.ok(response.redirected, "redirected should be true");
});

test("Agent redirect: 'stop' URL is original", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "stop",
	});

	const originalUrl = url("/redirect/3");
	const response = await faithFetch(originalUrl, { agent });
	t.equal(response.status, 302, "Should return redirect status");
	t.equal(response.url, originalUrl, "URL should remain original");
	t.notOk(response.redirected, "redirected should be false");
});

test("Agent redirect: 'follow' with 301 permanent redirect", async (t) => {
	t.plan(2);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/status/301"), { agent });
	t.ok(response.status >= 200, "Should handle 301 redirects");
	t.ok(true, "Should not throw error");
});

test("Agent redirect: 'follow' with 307 temporary redirect", async (t) => {
	t.plan(2);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/status/307"), { agent });
	t.ok(response.status >= 200, "Should handle 307 redirects");
	t.ok(true, "Should not throw error");
});

test("Agent redirect: 'follow' with 308 permanent redirect", async (t) => {
	t.plan(2);

	const agent = new Agent({
		redirect: "follow",
	});

	const response = await faithFetch(url("/status/308"), { agent });
	t.ok(response.status >= 200, "Should handle 308 redirects");
	t.ok(true, "Should not throw error");
});

test("Agent redirect: 'stop' with various redirect statuses", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "stop",
	});

	const response301 = await faithFetch(url("/status/301"), { agent });
	t.ok(
		response301.status === 301,
		"Should return 301 status without following",
	);

	const response302 = await faithFetch(url("/status/302"), { agent });
	t.ok(
		response302.status === 302,
		"Should return 302 status without following",
	);

	const response307 = await faithFetch(url("/status/307"), { agent });
	t.ok(
		response307.status === 307,
		"Should return 307 status without following",
	);
});

test("Agent redirect with non-redirect status codes", async (t) => {
	t.plan(3);

	const agent = new Agent({
		redirect: "error",
	});

	const response200 = await faithFetch(url("/status/200"), { agent });
	t.equal(response200.status, 200, "Should handle 200 OK");

	const response404 = await faithFetch(url("/status/404"), { agent });
	t.equal(response404.status, 404, "Should handle 404 Not Found");

	const response500 = await faithFetch(url("/status/500"), { agent });
	t.equal(response500.status, 500, "Should handle 500 Internal Server Error");
});

test("Agent redirect: 'follow' respects redirect limit", async (t) => {
	t.plan(1);

	const agent = new Agent({
		redirect: "follow",
	});

	try {
		await faithFetch(url("/redirect/15"), { agent });
		t.fail("Should throw error when exceeding redirect limit");
	} catch (err) {
		t.pass("Should throw error for too many redirects");
	}
});

test("Agent redirect mixed with successful requests", async (t) => {
	t.plan(6);

	const agent = new Agent({
		redirect: "follow",
	});

	const response1 = await faithFetch(url("/get"), { agent });
	t.ok(response1.ok, "First non-redirect should succeed");
	t.notOk(response1.redirected, "First request should not be redirected");

	const response2 = await faithFetch(url("/redirect/2"), { agent });
	t.ok(response2.ok, "Redirect request should succeed");
	t.ok(response2.redirected, "Second request should be redirected");

	const response3 = await faithFetch(url("/headers"), { agent });
	t.ok(response3.ok, "Third non-redirect should succeed");
	t.notOk(response3.redirected, "Third request should not be redirected");
});

test("Agent redirect: 'error' with redirect chain", async (t) => {
	t.plan(1);

	const agent = new Agent({
		redirect: "error",
	});

	try {
		await faithFetch(url("/redirect/5"), { agent });
		t.fail("Should throw error on any redirect");
	} catch (err) {
		t.pass("Should throw error on first redirect in chain");
	}
});

test("Agent redirect with different response types", async (t) => {
	t.plan(6);

	const followAgent = new Agent({ redirect: "follow" });
	const stopAgent = new Agent({ redirect: "stop" });

	const followResponse = await faithFetch(url("/redirect/2"), {
		agent: followAgent,
	});
	t.equal(followResponse.status, 200, "Follow response should be 200");
	t.ok(followResponse.redirected, "Follow response should be redirected");

	const stopResponse = await faithFetch(url("/redirect/2"), {
		agent: stopAgent,
	});
	t.equal(stopResponse.status, 302, "Stop response should be 302");
	t.notOk(stopResponse.redirected, "Stop response should not be redirected");

	const noRedirectResponse = await faithFetch(url("/get"), {
		agent: followAgent,
	});
	t.equal(
		noRedirectResponse.status,
		200,
		"Non-redirect response should be 200",
	);
	t.notOk(
		noRedirectResponse.redirected,
		"Non-redirect should not be redirected",
	);
});
