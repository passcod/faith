const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

// Helper to get header value (handles both string and array)
function getHeader(headers, name) {
	const value = headers[name];
	return Array.isArray(value) ? value[0] : value;
}

// Helper to get cookies from response (go-httpbin returns cookies at root level)
function getCookies(data) {
	// If data has typical httpbin fields, cookies are not at root level
	if (data.headers || data.url || data.method) {
		return {};
	}
	// Otherwise, the entire response is cookies (for /cookies endpoint)
	return data;
}

test("Agent with headers option sets default headers", async (t) => {
	t.plan(3);

	const agent = new Agent({
		headers: [
			{ name: "X-Custom-Header", value: "custom-value" },
			{ name: "X-Another-Header", value: "another-value" },
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch with custom headers");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Custom-Header"),
		"custom-value",
		"First custom header should be sent",
	);
	t.equal(
		getHeader(data.headers, "X-Another-Header"),
		"another-value",
		"Second custom header should be sent",
	);
});

test("Agent headers persist across multiple requests", async (t) => {
	t.plan(5);

	const agent = new Agent({
		headers: [{ name: "X-Persistent", value: "persistent-value" }],
	});

	const response1 = await faithFetch(url("/headers"), { agent });
	const response2 = await faithFetch(url("/get"), { agent });

	t.ok(response1.ok, "First request should succeed");
	t.ok(response2.ok, "Second request should succeed");

	const data1 = await response1.json();
	const data2 = await response2.json();

	t.equal(
		getHeader(data1.headers, "X-Persistent"),
		"persistent-value",
		"First request should have custom header",
	);
	t.equal(
		getHeader(data2.headers, "X-Persistent"),
		"persistent-value",
		"Second request should have custom header",
	);
	t.equal(
		getHeader(data1.headers, "X-Persistent"),
		getHeader(data2.headers, "X-Persistent"),
		"Both requests should have the same header value",
	);
});

test("Request-level headers override agent headers", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "X-Custom", value: "agent-value" }],
	});

	const response = await faithFetch(url("/headers"), {
		agent,
		headers: {
			"X-Custom": "request-value",
		},
	});

	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Custom"),
		"request-value",
		"Request-level header should override agent header",
	);
});

test("Agent headers work with sensitive flag", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [
			{
				name: "Authorization",
				value: "Bearer secret-token",
				sensitive: true,
			},
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch with sensitive header");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "Authorization"),
		"Bearer secret-token",
		"Sensitive header should be sent",
	);
});

test("Agent headers without sensitive flag work", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [
			{ name: "X-Public", value: "public-value", sensitive: false },
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Public"),
		"public-value",
		"Non-sensitive header should be sent",
	);
});

test("Agent headers with omitted sensitive flag work", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "X-Default", value: "default-value" }],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Default"),
		"default-value",
		"Header without sensitive flag should be sent",
	);
});

test("Multiple agents can have different headers", async (t) => {
	t.plan(4);

	const agent1 = new Agent({
		headers: [{ name: "X-Agent", value: "agent1" }],
	});
	const agent2 = new Agent({
		headers: [{ name: "X-Agent", value: "agent2" }],
	});

	const response1 = await faithFetch(url("/headers"), { agent: agent1 });
	const response2 = await faithFetch(url("/headers"), { agent: agent2 });

	t.ok(response1.ok, "First agent request should succeed");
	t.ok(response2.ok, "Second agent request should succeed");

	const data1 = await response1.json();
	const data2 = await response2.json();

	t.equal(
		getHeader(data1.headers, "X-Agent"),
		"agent1",
		"First agent should send its header",
	);
	t.equal(
		getHeader(data2.headers, "X-Agent"),
		"agent2",
		"Second agent should send its header",
	);
});

test("Agent with empty headers array", async (t) => {
	t.plan(1);

	const agent = new Agent({ headers: [] });
	const response = await faithFetch(url("/headers"), { agent });

	t.ok(response.ok, "Should successfully fetch with empty headers array");
});

test("Agent without headers option", async (t) => {
	t.plan(1);

	const agent = new Agent();
	const response = await faithFetch(url("/headers"), { agent });

	t.ok(response.ok, "Should successfully fetch without headers option");
});

test("Agent headers with multiple values", async (t) => {
	t.plan(4);

	const agent = new Agent({
		headers: [
			{ name: "X-Header-1", value: "value1" },
			{ name: "X-Header-2", value: "value2" },
			{ name: "X-Header-3", value: "value3" },
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Header-1"),
		"value1",
		"First header should be set",
	);
	t.equal(
		getHeader(data.headers, "X-Header-2"),
		"value2",
		"Second header should be set",
	);
	t.equal(
		getHeader(data.headers, "X-Header-3"),
		"value3",
		"Third header should be set",
	);
});

test("Agent headers work with other agent options", async (t) => {
	t.plan(4);

	const agent = new Agent({
		headers: [{ name: "X-Custom", value: "test" }],
		userAgent: "CustomAgent/1.0",
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Custom"),
		"test",
		"Custom header should be sent",
	);
	t.equal(
		getHeader(data.headers, "User-Agent"),
		"CustomAgent/1.0",
		"Custom user agent should be set",
	);
	t.ok(
		getHeader(data.headers, "X-Custom") &&
			getHeader(data.headers, "User-Agent"),
		"Both options should work together",
	);
});

test("Agent headers with cookies option", async (t) => {
	t.plan(4);

	const agent = new Agent({
		headers: [{ name: "X-Custom", value: "with-cookies" }],
		cookies: true,
	});

	const cookiesUrl = url("/cookies");
	const headersUrl = url("/headers");
	agent.addCookie(cookiesUrl, "session=test");

	const response1 = await faithFetch(cookiesUrl, { agent });
	t.ok(response1.ok, "Should successfully fetch cookies endpoint");

	const data1 = await response1.json();
	const cookies1 = getCookies(data1);
	t.equal(cookies1.session, "test", "Cookie should be sent");

	const response2 = await faithFetch(headersUrl, { agent });
	const data2 = await response2.json();
	t.equal(
		getHeader(data2.headers, "X-Custom"),
		"with-cookies",
		"Custom header should be sent",
	);
	t.ok(response2.ok, "Should successfully fetch headers endpoint");
});

test("Invalid header names are silently omitted", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [
			{ name: "Valid-Header", value: "valid" },
			{ name: "Invalid Header With Spaces", value: "invalid" },
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch despite invalid header");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "Valid-Header"),
		"valid",
		"Valid header should be sent",
	);
});

test("Invalid header values are silently omitted", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [
			{ name: "X-Valid", value: "valid-value" },
			{ name: "X-Invalid", value: "invalid\nvalue\rwith\r\ncontrol" },
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch despite invalid header value");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Valid"),
		"valid-value",
		"Valid header should be sent",
	);
});

test("Agent headers with special characters in values", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [
			{
				name: "X-Special",
				value: "value-with-dashes_and_underscores.dots",
			},
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Special"),
		"value-with-dashes_and_underscores.dots",
		"Special characters in value should be preserved",
	);
});

test("Agent headers with empty value", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "X-Empty", value: "" }],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch with empty value");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Empty"),
		"",
		"Empty header value should be sent",
	);
});

test("Agent headers with Content-Type", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "Content-Type", value: "application/json" }],
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: JSON.stringify({ test: "data" }),
		agent,
	});

	t.ok(response.ok, "Should successfully POST");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "Content-Type"),
		"application/json",
		"Content-Type from agent should be used",
	);
});

test("Request Content-Type overrides agent Content-Type", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "Content-Type", value: "text/plain" }],
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ test: "data" }),
		agent,
	});

	t.ok(response.ok, "Should successfully POST");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "Content-Type"),
		"application/json",
		"Request Content-Type should override agent Content-Type",
	);
});

test("Agent headers work with parallel requests", async (t) => {
	t.plan(6);

	const agent = new Agent({
		headers: [{ name: "X-Parallel", value: "parallel-value" }],
	});

	const promises = [
		faithFetch(url("/headers"), { agent }),
		faithFetch(url("/get"), { agent }),
		faithFetch(url("/status/200"), { agent }),
	];

	const responses = await Promise.all(promises);

	t.ok(responses[0].ok, "First parallel request should succeed");
	t.ok(responses[1].ok, "Second parallel request should succeed");
	t.ok(responses[2].ok, "Third parallel request should succeed");

	const data1 = await responses[0].json();
	const data2 = await responses[1].json();

	t.equal(
		getHeader(data1.headers, "X-Parallel"),
		"parallel-value",
		"First request should have custom header",
	);
	t.equal(
		getHeader(data2.headers, "X-Parallel"),
		"parallel-value",
		"Second request should have custom header",
	);
	t.equal(responses[2].status, 200, "Third request should have status 200");
});

test("Agent headers with Accept header", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "Accept", value: "application/json, text/plain" }],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "Accept"),
		"application/json, text/plain",
		"Accept header should be sent",
	);
});

test("Agent headers with Authorization header marked sensitive", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [
			{
				name: "Authorization",
				value: "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
				sensitive: true,
			},
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "Authorization"),
		"Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
		"Authorization header should be sent",
	);
});

test("Agent headers with case variations", async (t) => {
	t.plan(4);

	const agent = new Agent({
		headers: [
			{ name: "x-lowercase", value: "lowercase" },
			{ name: "X-UPPERCASE", value: "uppercase" },
			{ name: "X-MixedCase", value: "mixed" },
		],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.ok(
		getHeader(data.headers, "x-lowercase") ||
			getHeader(data.headers, "X-Lowercase"),
		"Lowercase header should be sent",
	);
	t.ok(
		getHeader(data.headers, "X-UPPERCASE") ||
			getHeader(data.headers, "X-Uppercase"),
		"Uppercase header should be sent",
	);
	t.ok(
		getHeader(data.headers, "X-MixedCase") ||
			getHeader(data.headers, "X-Mixedcase"),
		"Mixed case header should be sent",
	);
});

test("Agent headers combined with request headers", async (t) => {
	t.plan(3);

	const agent = new Agent({
		headers: [{ name: "X-Agent-Header", value: "from-agent" }],
	});

	const response = await faithFetch(url("/headers"), {
		agent,
		headers: {
			"X-Request-Header": "from-request",
		},
	});

	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Agent-Header"),
		"from-agent",
		"Agent header should be sent",
	);
	t.equal(
		getHeader(data.headers, "X-Request-Header"),
		"from-request",
		"Request header should be sent",
	);
});

test("Agent headers with numeric values", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "X-Numeric", value: "12345" }],
	});

	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	t.equal(
		getHeader(data.headers, "X-Numeric"),
		"12345",
		"Numeric string value should be sent",
	);
});

test("Agent with many headers", async (t) => {
	t.plan(6);

	const headers = [];
	for (let i = 1; i <= 5; i++) {
		headers.push({ name: `X-Header-${i}`, value: `value-${i}` });
	}

	const agent = new Agent({ headers });
	const response = await faithFetch(url("/headers"), { agent });
	t.ok(response.ok, "Should successfully fetch with many headers");

	const data = await response.json();
	for (let i = 1; i <= 5; i++) {
		t.equal(
			getHeader(data.headers, `X-Header-${i}`),
			`value-${i}`,
			`Header ${i} should be sent`,
		);
	}
});
