const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

// spec:RESP#request-timing

// The origin echoes the query it is given back as response headers, so a test states the header
// value it wants an origin to have sent. `%3B` and friends keep the metric's own delimiters out
// of the query string's.
function timingUrl(...values) {
	const query = values
		.map((value) => `Server-Timing=${encodeURIComponent(value)}`)
		.join("&");
	return url(`/response-headers?${query}`);
}

async function serverTiming(...values) {
	const response = await faithFetch(timingUrl(...values));
	await response.text();
	const timing = await response.timing;
	return timing.serverTiming;
}

test("serverTiming reports the metrics an origin sent", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming('miss, db;dur=53, cache;desc="Cache Read";dur=23.2'),
		[
			{ name: "miss", duration: 0, description: "" },
			{ name: "db", duration: 53, description: "" },
			{ name: "cache", duration: 23.2, description: "Cache Read" },
		],
		"metrics should read in the order the header lists them",
	);
});

test("serverTiming is empty for a response without the header", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/get"));
	await response.text();
	const timing = await response.timing;

	t.deepEqual(timing.serverTiming, [], "serverTiming should be empty");
});

test("serverTiming gathers repeated header lines", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming("db;dur=1", "cache;desc=hit"),
		[
			{ name: "db", duration: 1, description: "" },
			{ name: "cache", duration: 0, description: "hit" },
		],
		"metrics should gather across header lines",
	);
});

test("serverTiming keeps a metric name reported more than once", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming("db;dur=1, db;dur=2", "db;dur=3"),
		[
			{ name: "db", duration: 1, description: "" },
			{ name: "db", duration: 2, description: "" },
			{ name: "db", duration: 3, description: "" },
		],
		"each occurrence should be its own entry",
	);
});

test("serverTiming reads a quoted description whole", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming('db;desc="a, b; c";dur=1, next;dur=2'),
		[
			{ name: "db", duration: 1, description: "a, b; c" },
			{ name: "next", duration: 2, description: "" },
		],
		"a comma or semicolon inside quotes should not end the metric",
	);
});

test("serverTiming defaults the parameters a metric omits", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming("total, blank;dur, empty;dur=;desc="),
		[
			{ name: "total", duration: 0, description: "" },
			{ name: "blank", duration: 0, description: "" },
			{ name: "empty", duration: 0, description: "" },
		],
		"a metric naming itself alone should still appear",
	);
});

test("serverTiming reads what a malformed dur reports", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming("bad;dur=abc, trailing;dur=53ms, exp;dur=1e3"),
		[
			{ name: "bad", duration: 0, description: "" },
			{ name: "trailing", duration: 53, description: "" },
			{ name: "exp", duration: 1000, description: "" },
		],
		"a dur that is not a number should read 0",
	);
});

test("serverTiming takes the first of a parameter given twice", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming("db;dur=1;dur=2;desc=first"),
		[{ name: "db", duration: 1, description: "first" }],
		"a repeated parameter should not disturb the ones after it",
	);
});

test("serverTiming drops a metric with no name", async (t) => {
	t.plan(1);

	t.deepEqual(
		await serverTiming(", ;dur=1, db;dur=2"),
		[{ name: "db", duration: 2, description: "" }],
		"the rest of the list should stand",
	);
});

test("serverTiming is empty for a header that is not valid UTF-8", async (t) => {
	t.plan(2);

	// `%FF` is no UTF-8 sequence, and the origin sends the byte as it stands, so the response
	// arrives carrying a header value Faith drops rather than reads lossily.
	const response = await faithFetch(
		url("/response-headers?Server-Timing=db%3Bdesc%3D%FFx%3Bdur%3D1"),
	);
	await response.text();
	const timing = await response.timing;

	t.equal(
		response.headers.get("server-timing"),
		null,
		"the header itself should have been dropped",
	);
	t.deepEqual(timing.serverTiming, [], "serverTiming should be empty");
});

test("serverTiming reads the metrics a cached response stored", async (t) => {
	t.plan(3);

	const agent = new Agent({ cache: { store: "memory" } });
	const target = url(
		`/response-headers?Cache-Control=${encodeURIComponent("max-age=60")}&Server-Timing=${encodeURIComponent("db;dur=53")}`,
	);
	const metrics = [{ name: "db", duration: 53, description: "" }];

	const first = await faithFetch(target, { agent });
	await first.text();
	const firstTiming = await first.timing;
	t.deepEqual(firstTiming.serverTiming, metrics, "the network response");

	const second = await faithFetch(target, { agent });
	await second.text();
	const secondTiming = await second.timing;
	t.equal(secondTiming.deliveryType, "cache", "the second should be a hit");
	t.deepEqual(
		secondTiming.serverTiming,
		metrics,
		"a hit should report the metrics its stored headers carry",
	);

	agent.close();
});

test("serverTiming survives serialisation", async (t) => {
	t.plan(1);

	const response = await faithFetch(timingUrl('db;dur=53;desc="db query"'));
	await response.text();
	const timing = await response.timing;
	const json = JSON.parse(JSON.stringify(timing));

	t.deepEqual(
		json.serverTiming,
		[{ name: "db", duration: 53, description: "db query" }],
		"JSON should carry the metrics",
	);
});
