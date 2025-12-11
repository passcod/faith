const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Response.peer exists", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/get"));
	t.ok(response.peer, "peer property should exist on Response");
});

test("Response.peer is an object", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/get"));
	t.equal(typeof response.peer, "object", "peer should be an object");
});

test("Response.peer.address is available for HTTP requests", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/get"));

	t.ok(response.peer.address, "peer.address should be present");
	t.equal(
		typeof response.peer.address,
		"string",
		"peer.address should be a string",
	);
});

test("Response.peer.address format includes port", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/get"));

	t.match(
		response.peer.address,
		/:\d+$/,
		"peer.address should include port number",
	);
});

test("Response.peer.certificate is null for HTTP requests", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/get"));

	t.equal(
		response.peer.certificate,
		null,
		"peer.certificate should be null for HTTP",
	);
});

test("Response.peer persists across multiple requests", async (t) => {
	t.plan(4);

	const response1 = await faithFetch(url("/get"));
	const response2 = await faithFetch(url("/headers"));

	t.ok(response1.peer, "First response should have peer");
	t.ok(response2.peer, "Second response should have peer");
	t.ok(response1.peer.address, "First response should have peer.address");
	t.ok(response2.peer.address, "Second response should have peer.address");
});

test("Response.peer is available after reading body", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/get"));
	await response.json();

	t.ok(response.peer, "peer should still be available after reading body");
	t.ok(
		response.peer.address,
		"peer.address should still be available after reading body",
	);
});

test("Response.peer for POST request", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: JSON.stringify({ test: "data" }),
		headers: { "Content-Type": "application/json" },
	});

	t.ok(response.peer, "peer should exist for POST request");
	t.ok(response.peer.address, "peer.address should exist for POST request");
});

test("Response.peer for different HTTP methods", async (t) => {
	t.plan(8);

	const methods = [
		{ method: "GET", url: "/get" },
		{ method: "POST", url: "/post", body: "test" },
		{ method: "PUT", url: "/put", body: "test" },
		{ method: "DELETE", url: "/delete" },
	];

	for (const { method, url: path, body } of methods) {
		const response = await faithFetch(url(path), { method, body });
		t.ok(response.peer, `peer should exist for ${method} request`);
		t.ok(
			response.peer.address,
			`peer.address should exist for ${method} request`,
		);
	}
});

test("Response.peer for error responses", async (t) => {
	t.plan(6);

	const response1 = await faithFetch(url("/status/404"));
	t.ok(response1.peer, "peer should exist for 404 response");
	t.ok(response1.peer.address, "peer.address should exist for 404 response");

	const response2 = await faithFetch(url("/status/500"));
	t.ok(response2.peer, "peer should exist for 500 response");
	t.ok(response2.peer.address, "peer.address should exist for 500 response");

	const response3 = await faithFetch(url("/status/403"));
	t.ok(response3.peer, "peer should exist for 403 response");
	t.ok(response3.peer.address, "peer.address should exist for 403 response");
});

test("Response.peer for redirected requests", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/redirect/2"));

	t.ok(response.peer, "peer should exist for redirected request");
	t.ok(
		response.peer.address,
		"peer.address should exist for redirected request",
	);
	t.equal(
		response.peer.certificate,
		null,
		"peer.certificate should be null for HTTP redirect",
	);
});

test("Response.peer with streaming body", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/stream/20"));

	t.ok(response.peer, "peer should exist for streaming response");
	t.ok(
		response.peer.address,
		"peer.address should exist for streaming response",
	);

	await response.text();
});

test("Response.peer for parallel requests", async (t) => {
	t.plan(6);

	const promises = [
		faithFetch(url("/get")),
		faithFetch(url("/headers")),
		faithFetch(url("/status/200")),
	];

	const responses = await Promise.all(promises);

	for (let i = 0; i < responses.length; i++) {
		t.ok(responses[i].peer, `Response ${i + 1} should have peer`);
		t.ok(
			responses[i].peer.address,
			`Response ${i + 1} should have peer.address`,
		);
	}
});

test("Response.peer with timeout option", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/delay/1"), { timeout: 5000 });

	t.ok(response.peer, "peer should exist for delayed request");
	t.ok(
		response.peer.address,
		"peer.address should exist for delayed request",
	);
});

test("Response.peer is read-only", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/get"));
	const originalAddress = response.peer.address;

	try {
		response.peer = { address: "modified" };
	} catch (err) {}

	t.equal(
		response.peer.address,
		originalAddress,
		"peer property should not be modifiable",
	);
});

test("Response.peer.address is not empty", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/get"));

	t.ok(response.peer.address, "peer.address should not be null/undefined");
	t.ok(
		response.peer.address.length > 0,
		"peer.address should not be empty string",
	);
});

test("Response.peer with custom headers", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/headers"), {
		headers: {
			"X-Custom-Header": "test-value",
		},
	});

	t.ok(response.peer, "peer should exist with custom headers");
	t.ok(
		response.peer.address,
		"peer.address should exist with custom headers",
	);
});

test("Response.peer with Agent", async (t) => {
	t.plan(2);

	const { Agent } = require("../wrapper.js");
	const agent = new Agent();

	const response = await faithFetch(url("/get"), { agent });

	t.ok(response.peer, "peer should exist when using Agent");
	t.ok(response.peer.address, "peer.address should exist when using Agent");
});

test("Response.peer available on Faith response", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/get"));

	t.ok(response.peer, "peer should exist on Faith response");
	t.ok(response.peer.address, "peer.address should exist on Faith response");
});

test("Response.peer structure is consistent", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/get"));

	t.ok("address" in response.peer, "peer should have address property");
	t.ok(
		"certificate" in response.peer,
		"peer should have certificate property",
	);
	t.equal(
		Object.keys(response.peer).length,
		2,
		"peer should have exactly 2 properties",
	);
});
