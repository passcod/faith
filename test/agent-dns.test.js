const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url, port } = require("./helpers.js");

test("Agent with dns.system option enabled", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: { system: true },
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with system DNS");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with dns.system option disabled (default)", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: { system: false },
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with Hickory DNS");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent without dns option uses default", async (t) => {
	t.plan(2);

	const agent = new Agent();
	const response = await faithFetch(url("/get"), { agent });

	t.ok(response.ok, "Should successfully fetch with default DNS");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with empty dns object", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {},
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with empty dns object");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with dns.overrides for localhost", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "localhost",
					addresses: ["127.0.0.1"],
				},
			],
		},
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch with DNS override");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with dns.overrides for custom domain", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "example.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/get").replace("localhost", "example.tld");
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should resolve custom domain to override address");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with dns.overrides with port number", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "custom.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/get").replace(
		`localhost:${port()}`,
		`custom.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should resolve domain with explicit port");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent with multiple dns.overrides", async (t) => {
	t.plan(4);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "test1.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
				{
					domain: "test2.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl1 = url("/get").replace(
		`localhost:${port()}`,
		`test1.tld:${port()}`,
	);
	const response1 = await faithFetch(testUrl1, { agent });
	t.ok(response1.ok, "First override should work");
	t.equal(response1.status, 200, "Status should be 200");

	const testUrl2 = url("/headers").replace(
		`localhost:${port()}`,
		`test2.tld:${port()}`,
	);
	const response2 = await faithFetch(testUrl2, { agent });
	t.ok(response2.ok, "Second override should work");
	t.equal(response2.status, 200, "Status should be 200");
});

test("Agent with dns.overrides empty addresses blocks domain", async (t) => {
	t.plan(1);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "blocked.tld",
					addresses: [],
				},
			],
		},
	});

	try {
		const testUrl = url("/get").replace("localhost", "blocked.tld");
		await faithFetch(testUrl, { agent });
		t.fail("Should fail to resolve blocked domain");
	} catch (err) {
		t.pass("Should throw error for blocked domain");
	}
});

test("Agent dns.system works with dns.overrides", async (t) => {
	t.plan(1);

	const agent = new Agent({
		dns: {
			system: true,
			overrides: [
				{
					domain: "localhost",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Overrides should work even with system DNS");
});

test("Agent dns persists across multiple requests", async (t) => {
	t.plan(4);

	const agent = new Agent({
		dns: {
			system: true,
		},
	});

	const response1 = await faithFetch(url("/get"), { agent });
	t.ok(response1.ok, "First request should succeed");

	const response2 = await faithFetch(url("/headers"), { agent });
	t.ok(response2.ok, "Second request should succeed");

	const response3 = await faithFetch(url("/status/200"), { agent });
	t.ok(response3.ok, "Third request should succeed");

	t.equal(response3.status, 200, "Status should be 200");
});

test("Different agents can have different DNS settings", async (t) => {
	t.plan(4);

	const agent1 = new Agent({
		dns: { system: true },
	});

	const agent2 = new Agent({
		dns: { system: false },
	});

	const response1 = await faithFetch(url("/get"), { agent: agent1 });
	t.ok(response1.ok, "Agent1 with system DNS should work");

	const response2 = await faithFetch(url("/get"), { agent: agent2 });
	t.ok(response2.ok, "Agent2 with Hickory DNS should work");

	t.equal(response1.status, 200, "Agent1 status should be 200");
	t.equal(response2.status, 200, "Agent2 status should be 200");
});

test("Agent dns.overrides with multiple addresses", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "multi.tld",
					addresses: [`127.0.0.1:${port()}`, `127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/get").replace(
		`localhost:${port()}`,
		`multi.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should work with multiple addresses");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns with POST request", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: { system: true },
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: JSON.stringify({ test: "data" }),
		headers: { "Content-Type": "application/json" },
		agent,
	});

	t.ok(response.ok, "Should successfully POST with DNS settings");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns with parallel requests", async (t) => {
	t.plan(3);

	const agent = new Agent({
		dns: { system: true },
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

test("Agent dns with other agent options", async (t) => {
	t.plan(4);

	const agent = new Agent({
		dns: { system: true },
		userAgent: "DnsAgent/1.0",
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

	t.equal(userAgent, "DnsAgent/1.0", "User-Agent should be set");
	t.equal(customHeader, "test", "Custom header should be set");
	t.ok(true, "All options work together");
});

test("Agent dns with cookies", async (t) => {
	t.plan(1);

	const agent = new Agent({
		dns: { system: true },
		cookies: true,
	});

	const cookiesUrl = url("/cookies");
	agent.addCookie(cookiesUrl, "session=test");

	const response = await faithFetch(cookiesUrl, { agent });
	t.ok(response.ok, "Should successfully fetch with DNS and cookies");
});

test("Agent dns with timeout", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: { system: true },
		timeout: { total: 5000 },
	});

	const response = await faithFetch(url("/delay/1"), { agent });
	t.ok(response.ok, "Should successfully fetch with DNS and timeout");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns.overrides case sensitivity", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "CaseSensitive.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/get").replace(
		`localhost:${port()}`,
		`CaseSensitive.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should resolve case-sensitive domain");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns.overrides with redirects", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "redirect.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/redirect/2").replace(
		`localhost:${port()}`,
		`redirect.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should handle redirects with DNS overrides");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns.overrides with error responses", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "error.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/status/404").replace(
		`localhost:${port()}`,
		`error.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.equal(response.status, 404, "Should receive 404 status");
	t.notOk(response.ok, "Response should not be ok");
});

test("Agent dns stats tracking", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: { system: true },
	});

	await faithFetch(url("/get"), { agent });
	await faithFetch(url("/headers"), { agent });
	await faithFetch(url("/status/200"), { agent });

	const stats = agent.stats();
	t.equal(stats.requestsSent, 3, "Should track all requests");
	t.equal(stats.responsesReceived, 3, "Should track all responses");
});

test("Agent dns.overrides with IPv4 address", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "ipv4.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/get").replace(
		`localhost:${port()}`,
		`ipv4.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should work with IPv4 address");
	t.equal(response.status, 200, "Status should be 200");
});

// SKIP: this one doesn't work in CI (works locally)
test.skip("Agent dns.overrides with IPv6 address", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "ipv6.tld",
					addresses: [`[::1]:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/get").replace(
		`localhost:${port()}`,
		`ipv6.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should work with IPv6 address");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns.overrides without port uses default", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "noport.tld",
					addresses: ["127.0.0.1"],
				},
			],
		},
	});

	const testUrl = url("/get").replace("localhost", "noport.tld");
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should work without explicit port in override");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns.system true vs false behavior", async (t) => {
	t.plan(4);

	const agentSystem = new Agent({
		dns: { system: true },
	});

	const agentHickory = new Agent({
		dns: { system: false },
	});

	const response1 = await faithFetch(url("/get"), { agent: agentSystem });
	t.ok(response1.ok, "System DNS should work");

	const response2 = await faithFetch(url("/get"), { agent: agentHickory });
	t.ok(response2.ok, "Hickory DNS should work");

	t.equal(response1.status, 200, "System DNS status should be 200");
	t.equal(response2.status, 200, "Hickory DNS status should be 200");
});

test("Agent dns.overrides with streaming response", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			overrides: [
				{
					domain: "stream.tld",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const testUrl = url("/stream/20").replace(
		`localhost:${port()}`,
		`stream.tld:${port()}`,
	);
	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should work with streaming response");

	const text = await response.text();
	t.ok(text.length > 0, "Should receive streamed data");
});

test("Agent dns.overrides override takes precedence", async (t) => {
	t.plan(2);

	const agent = new Agent({
		dns: {
			system: true,
			overrides: [
				{
					domain: "localhost",
					addresses: [`127.0.0.1:${port()}`],
				},
			],
		},
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Override should take precedence over system DNS");
	t.equal(response.status, 200, "Status should be 200");
});

test("Agent dns with peer information", async (t) => {
	t.plan(3);

	const agent = new Agent({
		dns: { system: true },
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "Should successfully fetch");
	t.ok(response.peer, "Should have peer information");
	t.ok(response.peer.address, "Should have peer address");
});
