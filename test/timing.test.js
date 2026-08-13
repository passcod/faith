const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

// spec:RESP#request-timing

test("Response.timing resolves to a PerformanceResourceTiming", async (t) => {
	t.plan(4);

	const response = await faithFetch(url("/get"));
	await response.text();
	const timing = await response.timing;

	t.ok(
		timing instanceof PerformanceResourceTiming,
		"timing should be a genuine PerformanceResourceTiming",
	);
	t.equal(timing.entryType, "resource", "entryType should be resource");
	t.equal(timing.initiatorType, "fetch", "initiatorType should be fetch");
	t.equal(timing.name, url("/get"), "name should be the final URL");
});

test("Response.timing phases are ordered and on the performance clock", async (t) => {
	t.plan(5);

	const before = performance.now();
	const response = await faithFetch(url("/get"));
	await response.text();
	const timing = await response.timing;
	const after = performance.now();

	t.ok(timing.fetchStart >= before, "fetchStart should be after the start");
	t.ok(timing.responseEnd <= after, "responseEnd should be before the end");
	t.ok(
		timing.finalResponseHeadersStart >= timing.fetchStart,
		"headers should arrive no earlier than the fetch started",
	);
	t.ok(
		timing.responseEnd >= timing.finalResponseHeadersStart,
		"the body should end no earlier than the headers arrived",
	);
	t.equal(
		timing.duration,
		timing.responseEnd - timing.startTime,
		"duration should be responseEnd less startTime",
	);
});

test("Response.timing derives responseStart from the final headers", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/get"));
	await response.text();
	const timing = await response.timing;

	t.equal(
		timing.firstInterimResponseStart,
		0,
		"no interim response should be reported",
	);
	t.equal(
		timing.responseStart,
		timing.finalResponseHeadersStart,
		"responseStart should fall through to the final headers",
	);
});

test("Response.timing reads 0 for the phases Faith does not observe", async (t) => {
	const unobserved = [
		"redirectStart",
		"redirectEnd",
		"domainLookupStart",
		"domainLookupEnd",
		"connectStart",
		"connectEnd",
		"secureConnectionStart",
		"requestStart",
		"requestSent",
		"firstInterimResponseStart",
		"transferSize",
		"encodedBodySize",
		"decodedBodySize",
	];
	t.plan(unobserved.length + 1);

	const response = await faithFetch(url("/get"));
	await response.text();
	const timing = await response.timing;

	for (const field of unobserved) {
		t.equal(timing[field], 0, `${field} should read 0`);
	}
	t.deepEqual(timing.serverTiming, [], "serverTiming should be empty");
});

test("Response.timing reads empty for the browsing context fields", async (t) => {
	t.plan(6);

	const response = await faithFetch(url("/get"));
	await response.text();
	const timing = await response.timing;

	t.equal(timing.workerStart, 0, "workerStart should read 0");
	t.equal(
		timing.workerRouterEvaluationStart,
		0,
		"workerRouterEvaluationStart should read 0",
	);
	t.equal(
		timing.workerCacheLookupStart,
		0,
		"workerCacheLookupStart should read 0",
	);
	t.equal(
		timing.workerMatchedRouterSource,
		"",
		"workerMatchedRouterSource should read empty",
	);
	t.equal(
		timing.workerFinalRouterSource,
		"",
		"workerFinalRouterSource should read empty",
	);
	t.equal(
		timing.renderBlockingStatus,
		"non-blocking",
		"renderBlockingStatus should read non-blocking",
	);
});

test("Response.timing reports the response's own details", async (t) => {
	t.plan(4);

	const response = await faithFetch(url("/json"));
	await response.json();
	const timing = await response.timing;

	t.equal(timing.responseStatus, 200, "responseStatus should be the status");
	t.equal(
		timing.contentType,
		"application/json",
		"contentType should be the MIME essence, without parameters",
	);
	t.equal(
		timing.nextHopProtocol,
		"http/1.1",
		"nextHopProtocol should be the ALPN protocol ID",
	);
	t.equal(
		timing.deliveryType,
		"",
		"deliveryType should be empty for a network response",
	);
});

test("Response.timing reports the coding a decoded body arrived under", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/gzip"));
	await response.json();
	const timing = await response.timing;

	t.equal(
		response.headers.get("content-encoding"),
		null,
		"the decoded response should not carry Content-Encoding",
	);
	t.equal(
		timing.contentEncoding,
		"gzip",
		"contentEncoding should report the coding it arrived under",
	);
});

test("Response.timing reports connection reuse", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const first = await faithFetch(url("/get"), { agent });
	await first.text();
	const firstTiming = await first.timing;
	t.equal(
		firstTiming.reused,
		false,
		"the first request should not report reuse",
	);

	const second = await faithFetch(url("/get"), { agent });
	await second.text();
	const secondTiming = await second.timing;
	t.equal(
		secondTiming.reused,
		true,
		"a request on a pooled connection should report reuse",
	);

	agent.close();
});

test("Response.timing settles when the body is discarded", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/bytes/1024"));
	await response.discard();
	const timing = await response.timing;

	t.ok(timing.responseEnd > 0, "responseEnd should be recorded");
	t.ok(
		timing.responseEnd >= timing.finalResponseHeadersStart,
		"the body should end no earlier than the headers arrived",
	);
});

test("Response.timing settles for a response that cannot carry a body", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/status/204"));
	const timing = await response.timing;

	t.equal(response.status, 204, "the response should be 204");
	t.ok(timing.responseEnd > 0, "responseEnd should be recorded");
});

test("Response.timing is one entry per request, shared with clones", async (t) => {
	t.plan(3);

	// A URL of its own, so the timeline count below sees this request alone.
	const target = url("/get?timing=clone");
	const response = await faithFetch(target);
	const clone = response.clone();
	await Promise.all([response.text(), clone.text()]);

	const timing = await response.timing;
	const cloneTiming = await clone.timing;

	t.equal(timing, cloneTiming, "a clone should share the original's entry");
	t.equal(
		await response.timing,
		timing,
		"repeated access should yield the same entry",
	);
	t.equal(
		performance
			.getEntriesByType("resource")
			.filter((entry) => entry.name === target).length,
		1,
		"the request should contribute exactly one timeline entry",
	);
});

test("Response.timing joins the resource timeline", async (t) => {
	t.plan(2);

	const seen = [];
	const observer = new PerformanceObserver((list) => {
		seen.push(...list.getEntries());
	});
	observer.observe({ type: "resource" });

	const response = await faithFetch(url("/uuid"));
	await response.text();
	const timing = await response.timing;
	await new Promise((resolve) => setTimeout(resolve, 50));
	observer.disconnect();

	const observed = seen.filter((entry) => entry.name === url("/uuid"));
	t.equal(observed.length, 1, "an observer should receive the entry");
	t.equal(observed[0], timing, "the observed entry should be the same object");
});

test("Response.timing serialises every field it carries", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/get"));
	await response.text();
	const timing = await response.timing;
	const json = JSON.parse(JSON.stringify(timing));

	t.equal(
		typeof json.reused,
		"boolean",
		"JSON should carry the fields added beyond the platform's class",
	);
	t.equal(
		json.contentType,
		"application/json",
		"JSON should carry contentType",
	);
	t.equal(json.name, timing.name, "JSON should carry the inherited fields");
});
