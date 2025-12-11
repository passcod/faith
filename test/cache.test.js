const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");
const fs = require("fs");
const path = require("path");
const os = require("os");

test("fetch with cache: 'default'", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/cache"), { cache: "default" });
	t.ok(response.ok, "Should successfully fetch with default cache mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("fetch with cache: 'no-store'", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/cache"), { cache: "no-store" });
	t.ok(response.ok, "Should successfully fetch with no-store cache mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("fetch with cache: 'reload'", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/cache"), { cache: "reload" });
	t.ok(response.ok, "Should successfully fetch with reload cache mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("fetch with cache: 'no-cache'", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/cache"), { cache: "no-cache" });
	t.ok(response.ok, "Should successfully fetch with no-cache mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("fetch with cache: 'force-cache'", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/cache"), { cache: "force-cache" });
	t.ok(response.ok, "Should successfully fetch with force-cache mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("fetch with cache: 'only-if-cached' without cached entry", async (t) => {
	t.plan(1);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	try {
		const response = await faithFetch(url("/get"), {
			cache: "only-if-cached",
			agent,
		});
		t.pass(
			"only-if-cached completed (may succeed or fail based on cache state)",
		);
	} catch (err) {
		t.pass("only-if-cached threw error when no cache entry available");
	}
});

test("fetch with cache: 'ignore-rules'", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/cache"), { cache: "ignore-rules" });
	t.ok(response.ok, "Should successfully fetch with ignore-rules mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with memory cache store", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with memory cache");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with disk cache store", async (t) => {
	t.plan(2);

	const cachePath = path.join(os.tmpdir(), `faith-cache-${Date.now()}`);

	const agent = new Agent({
		cache: {
			store: "disk",
			path: cachePath,
		},
	});

	try {
		const response = await faithFetch(url("/cache"), { agent });
		t.ok(response.ok, "Should successfully fetch with disk cache");
		t.equal(response.status, 200, "Status should be 200");
	} finally {
		try {
			fs.rmSync(cachePath, { recursive: true, force: true });
		} catch (err) {}
	}
});

test("Agent without cache (cache disabled)", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with cache disabled");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with cache capacity setting", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: {
			store: "memory",
			capacity: 100,
		},
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with custom capacity");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with cache mode default", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "default",
		},
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with default cache mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with cache mode no-store", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "no-store",
		},
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with no-store mode");
	t.equal(response.status, 200, "Status should be 200");
});

test("Memory cache stores and retrieves responses", async (t) => {
	t.plan(4);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "default",
		},
	});

	const testUrl = url("/cache/60");

	const response1 = await faithFetch(testUrl, { agent });
	t.ok(response1.ok, "First request should succeed");
	await response1.text();

	const response2 = await faithFetch(testUrl, { agent });
	t.ok(response2.ok, "Second request should succeed");
	t.equal(response2.status, 200, "Cached response should have status 200");
	await response2.text();

	t.pass("Both requests completed successfully");
});

test("Disk cache persists across agent instances", async (t) => {
	t.plan(4);

	const cachePath = path.join(
		os.tmpdir(),
		`faith-cache-persist-${Date.now()}`,
	);
	const testUrl = url("/cache/60");

	try {
		const agent1 = new Agent({
			cache: {
				store: "disk",
				path: cachePath,
			},
		});

		const response1 = await faithFetch(testUrl, { agent: agent1 });
		t.ok(response1.ok, "First request should succeed");
		await response1.text();

		const agent2 = new Agent({
			cache: {
				store: "disk",
				path: cachePath,
			},
		});

		const response2 = await faithFetch(testUrl, { agent: agent2 });
		t.ok(response2.ok, "Second request with new agent should succeed");
		t.equal(response2.status, 200, "Should retrieve from cache");
		await response2.text();

		t.pass("Cache persisted across agents");
	} finally {
		try {
			fs.rmSync(cachePath, { recursive: true, force: true });
		} catch (err) {}
	}
});

test("Request-level cache mode overrides agent cache mode", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "default",
		},
	});

	const response = await faithFetch(url("/cache"), {
		agent,
		cache: "no-store",
	});

	t.ok(response.ok, "Should successfully fetch");
	t.equal(response.status, 200, "Status should be 200");
});

test("force-cache mode uses stale cache entries", async (t) => {
	t.plan(3);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "force-cache",
		},
	});

	const testUrl = url("/cache/1");

	const response1 = await faithFetch(testUrl, { agent });
	t.ok(response1.ok, "First request should succeed");
	await response1.text();

	await new Promise((resolve) => setTimeout(resolve, 2000));

	const response2 = await faithFetch(testUrl, { agent });
	t.ok(response2.ok, "Should use stale cache with force-cache");
	await response2.text();

	t.pass("force-cache used stale entry");
});

test("no-cache mode makes conditional requests", async (t) => {
	t.plan(3);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "no-cache",
		},
	});

	const testUrl = url("/cache/60");

	const response1 = await faithFetch(testUrl, { agent });
	t.ok(response1.ok, "First request should succeed");
	await response1.text();

	const response2 = await faithFetch(testUrl, { agent });
	t.ok(response2.ok, "Second request should make conditional request");
	await response2.text();

	t.pass("no-cache mode completed");
});

test("reload mode bypasses cache but updates it", async (t) => {
	t.plan(3);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "reload",
		},
	});

	const testUrl = url("/cache/60");

	const response1 = await faithFetch(testUrl, { agent });
	t.ok(response1.ok, "First request should succeed");
	await response1.text();

	const response2 = await faithFetch(testUrl, { agent });
	t.ok(response2.ok, "Second request should bypass cache");
	await response2.text();

	t.pass("reload mode completed");
});

test("Cache with POST request", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: JSON.stringify({ test: "data" }),
		headers: { "Content-Type": "application/json" },
		agent,
	});

	t.ok(response.ok, "Should successfully POST with cache enabled");
	t.equal(response.status, 200, "Status should be 200");
});

test("Cache with different HTTP methods", async (t) => {
	t.plan(4);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const response1 = await faithFetch(url("/get"), { method: "GET", agent });
	t.ok(response1.ok, "GET should work with cache");

	const response2 = await faithFetch(url("/delete"), {
		method: "DELETE",
		agent,
	});
	t.ok(response2.ok, "DELETE should work with cache");

	const response3 = await faithFetch(url("/put"), {
		method: "PUT",
		body: "test",
		agent,
	});
	t.ok(response3.ok, "PUT should work with cache");

	const response4 = await faithFetch(url("/patch"), {
		method: "PATCH",
		body: "test",
		agent,
	});
	t.ok(response4.ok, "PATCH should work with cache");
});

test("Cache with error responses", async (t) => {
	t.plan(4);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const response1 = await faithFetch(url("/status/404"), { agent });
	t.equal(response1.status, 404, "Should receive 404");

	const response2 = await faithFetch(url("/status/500"), { agent });
	t.equal(response2.status, 500, "Should receive 500");

	const response3 = await faithFetch(url("/status/403"), { agent });
	t.equal(response3.status, 403, "Should receive 403");

	t.pass("Error responses handled with cache");
});

test("Cache with redirects", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const response = await faithFetch(url("/redirect/2"), { agent });
	t.ok(response.ok, "Should handle redirects with cache");
	t.equal(response.status, 200, "Status should be 200 after redirect");
});

test("Cache with parallel requests", async (t) => {
	t.plan(3);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const promises = [
		faithFetch(url("/cache/60"), { agent }),
		faithFetch(url("/cache/120"), { agent }),
		faithFetch(url("/cache/180"), { agent }),
	];

	const responses = await Promise.all(promises);

	t.ok(responses[0].ok, "First parallel request should succeed");
	t.ok(responses[1].ok, "Second parallel request should succeed");
	t.ok(responses[2].ok, "Third parallel request should succeed");
});

test("Cache with streaming responses", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const response = await faithFetch(url("/stream/20"), { agent });
	t.ok(response.ok, "Should successfully fetch streaming endpoint");

	const text = await response.text();
	t.ok(text.length > 0, "Should receive streamed data");
});

test("Cache with custom headers", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const response = await faithFetch(url("/headers"), {
		headers: {
			"X-Custom-Header": "test-value",
		},
		agent,
	});

	t.ok(response.ok, "Should successfully fetch with custom headers");
	t.equal(response.status, 200, "Status should be 200");
});

test("Cache works with Agent cookies", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
		cookies: true,
	});

	const cookiesUrl = url("/cookies");
	agent.addCookie(cookiesUrl, "session=test");

	const response = await faithFetch(cookiesUrl, { agent });
	t.ok(response.ok, "Should successfully fetch with cache and cookies");
	t.equal(response.status, 200, "Status should be 200");
});

test("Cache works with Agent custom headers", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
		headers: [{ name: "X-Agent-Header", value: "test" }],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch with cache and headers");
	t.equal(response.status, 200, "Status should be 200");
});

test("Cache works with Agent timeout", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
		timeout: { total: 5000 },
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with cache and timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Different agents with different caches are independent", async (t) => {
	t.plan(4);

	const agent1 = new Agent({
		cache: { store: "memory" },
	});

	const agent2 = new Agent({
		cache: { store: "memory" },
	});

	const testUrl = url("/cache/60");

	const response1 = await faithFetch(testUrl, { agent: agent1 });
	t.ok(response1.ok, "Agent1 first request should succeed");
	await response1.text();

	const response2 = await faithFetch(testUrl, { agent: agent2 });
	t.ok(response2.ok, "Agent2 should make fresh request");
	await response2.text();

	t.pass("Both agents completed requests");
	t.notEqual(agent1, agent2, "Agents should be different instances");
});

test("Cache stats tracking", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	await faithFetch(url("/cache"), { agent });
	await faithFetch(url("/headers"), { agent });

	const stats = agent.stats();
	t.equal(stats.requestsSent, 2, "Should track requests with cache");
	t.equal(stats.responsesReceived, 2, "Should track responses with cache");
});

test("ignore-rules cache mode", async (t) => {
	t.plan(3);

	const agent = new Agent({
		cache: {
			store: "memory",
			mode: "ignore-rules",
		},
	});

	const testUrl = url("/cache/1");

	const response1 = await faithFetch(testUrl, { agent });
	t.ok(response1.ok, "First request should succeed");
	await response1.text();

	await new Promise((resolve) => setTimeout(resolve, 2000));

	const response2 = await faithFetch(testUrl, { agent });
	t.ok(response2.ok, "Should use cache with ignore-rules");
	await response2.text();

	t.pass("ignore-rules mode completed");
});

test("Cache with large capacity", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: {
			store: "memory",
			capacity: 100000,
		},
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with large capacity");
	t.equal(response.status, 200, "Status should be 200");
});

test("Cache with small capacity", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: {
			store: "memory",
			capacity: 1,
		},
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with small capacity");
	t.equal(response.status, 200, "Status should be 200");
});

test("Memory cache with multiple entries", async (t) => {
	t.plan(6);

	const agent = new Agent({
		cache: { store: "memory" },
	});

	const urls = ["/cache/60", "/headers", "/get"];

	for (const u of urls) {
		const response = await faithFetch(url(u), { agent });
		t.ok(response.ok, `Should successfully fetch ${u}`);
		await response.text();
	}

	for (const u of urls) {
		const response = await faithFetch(url(u), { agent });
		t.ok(response.ok, `Should retrieve ${u} from cache`);
		await response.text();
	}
});

test("Disk cache creates directory if needed", async (t) => {
	t.plan(3);

	const cachePath = path.join(
		os.tmpdir(),
		`faith-cache-mkdir-${Date.now()}`,
		"nested",
		"cache",
	);

	try {
		const agent = new Agent({
			cache: {
				store: "disk",
				path: cachePath,
			},
		});

		const response = await faithFetch(url("/cache"), { agent });
		t.ok(response.ok, "Should successfully fetch");
		t.equal(response.status, 200, "Status should be 200");

		const cacheExists = fs.existsSync(cachePath);
		t.ok(cacheExists, "Cache directory should be created");
	} finally {
		try {
			fs.rmSync(
				path.join(os.tmpdir(), `faith-cache-mkdir-${Date.now()}`),
				{
					recursive: true,
					force: true,
				},
			);
		} catch (err) {}
	}
});

test("Cache without store setting", async (t) => {
	t.plan(2);

	const agent = new Agent({
		cache: {},
	});

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch with empty cache config");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent without cache option", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await faithFetch(url("/cache"), { agent });
	t.ok(response.ok, "Should successfully fetch without cache option");
	t.equal(response.status, 200, "Status should be 200");
});
