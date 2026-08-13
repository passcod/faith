const test = require("tape");
const { ReadableStream } = require("stream/web");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { streamingAgent, url } = require("./helpers.js");

// Helper to get header value (handles both string and array)
function getHeader(headers, name) {
	const value = headers[name];
	return Array.isArray(value) ? value[0] : value;
}

// The `Priority` header the request arrived with, or undefined if it carried none.
async function sentPriority(options) {
	const response = await faithFetch(url("/headers"), options);
	const data = await response.json();
	return getHeader(data.headers, "Priority");
}

test("priority high sends an urgency below the default", async (t) => {
	t.plan(1);
	t.equal(await sentPriority({ priority: "high" }), "u=1");
});

test("priority low sends an urgency above the default", async (t) => {
	t.plan(1);
	t.equal(await sentPriority({ priority: "low" }), "u=5");
});

test("priority auto sends no header", async (t) => {
	t.plan(1);
	t.equal(await sentPriority({ priority: "auto" }), undefined);
});

test("no priority option sends no header", async (t) => {
	t.plan(1);
	t.equal(await sentPriority({}), undefined);
});

test("an unrecognised priority is ignored rather than rejected", async (t) => {
	t.plan(3);
	t.equal(await sentPriority({ priority: "urgent" }), undefined);
	t.equal(await sentPriority({ priority: "HIGH" }), undefined);
	t.equal(await sentPriority({ priority: "" }), undefined);
});

test("a Priority header on the request wins over the option", async (t) => {
	t.plan(2);

	t.equal(
		await sentPriority({ priority: "high", headers: { Priority: "u=7, i" } }),
		"u=7, i",
	);
	t.equal(
		await sentPriority({ priority: "low", headers: { priority: "u=0" } }),
		"u=0",
		"header name matching is case-insensitive",
	);
});

test("an agent default Priority header wins over the option", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "Priority", value: "u=2" }],
	});

	t.equal(await sentPriority({ agent, priority: "high" }), "u=2");
	t.equal(
		await sentPriority({ agent, priority: "auto" }),
		"u=2",
		"the agent default is sent whatever the option says",
	);
});

test("a request Priority header wins over the agent default", async (t) => {
	t.plan(1);

	const agent = new Agent({
		headers: [{ name: "Priority", value: "u=2" }],
	});

	t.equal(
		await sentPriority({
			agent,
			priority: "high",
			headers: { Priority: "u=6" },
		}),
		"u=6",
	);
});

// The streaming path hands the body to the native binding separately, so the option
// travels a different route through the wrapper than it does on a buffered request.
test("priority applies to a request with a streaming body", async (t) => {
	t.plan(1);

	const stream = new ReadableStream({
		start(controller) {
			controller.enqueue(new TextEncoder().encode("payload"));
			controller.close();
		},
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: stream,
		duplex: "half",
		agent: streamingAgent(),
		priority: "high",
	});
	const data = await response.json();

	t.equal(getHeader(data.headers, "Priority"), "u=1");
});

test("unrelated agent default headers leave the option in effect", async (t) => {
	t.plan(2);

	const agent = new Agent({
		headers: [{ name: "X-Custom-Header", value: "custom-value" }],
	});

	const response = await faithFetch(url("/headers"), {
		agent,
		priority: "high",
	});
	const data = await response.json();

	t.equal(getHeader(data.headers, "Priority"), "u=1");
	t.equal(getHeader(data.headers, "X-Custom-Header"), "custom-value");
});
