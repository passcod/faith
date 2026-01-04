const test = require("tape");
const { fetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("bodiesStarted and bodiesFinished are 0 initially", async (t) => {
	t.plan(2);

	const agent = new Agent();
	const stats = agent.stats();

	t.equal(stats.bodiesStarted, 0, "bodiesStarted should be 0 initially");
	t.equal(stats.bodiesFinished, 0, "bodiesFinished should be 0 initially");
});

test("text() increments both bodiesStarted and bodiesFinished", async (t) => {
	t.plan(4);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent });
	await response.text();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "bodiesStarted should be 1 after text()");
	t.equal(stats.bodiesFinished, 1, "bodiesFinished should be 1 after text()");
	t.equal(
		stats.bodiesStarted - stats.bodiesFinished,
		0,
		"no bodies should be holding connections open",
	);
	t.ok(
		stats.bodiesStarted === stats.bodiesFinished,
		"bodies should be balanced",
	);
});

test("json() increments both bodiesStarted and bodiesFinished", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent });
	await response.json();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "bodiesStarted should be 1 after json()");
	t.equal(stats.bodiesFinished, 1, "bodiesFinished should be 1 after json()");
});

test("bytes() increments both bodiesStarted and bodiesFinished", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent });
	await response.bytes();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "bodiesStarted should be 1 after bytes()");
	t.equal(
		stats.bodiesFinished,
		1,
		"bodiesFinished should be 1 after bytes()",
	);
});

test("blob() increments both bodiesStarted and bodiesFinished", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent });
	await response.blob();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "bodiesStarted should be 1 after blob()");
	t.equal(stats.bodiesFinished, 1, "bodiesFinished should be 1 after blob()");
});

test("accessing body property starts but does not finish body", async (t) => {
	t.plan(3);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent });
	const stream = response.body;
	t.ok(stream, "body stream should exist");

	const stats = agent.stats();
	t.equal(
		stats.bodiesStarted,
		1,
		"bodiesStarted should be 1 after accessing body",
	);
	t.equal(
		stats.bodiesFinished,
		0,
		"bodiesFinished should be 0 (stream not consumed)",
	);
});

test("fully consuming body stream finishes body", async (t) => {
	t.plan(3);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent });
	const stream = response.body;
	const reader = stream.getReader();

	while (true) {
		const { done } = await reader.read();
		if (done) break;
	}
	reader.releaseLock();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "bodiesStarted should be 1");
	t.equal(
		stats.bodiesFinished,
		1,
		"bodiesFinished should be 1 after consuming stream",
	);
	t.equal(
		stats.bodiesStarted - stats.bodiesFinished,
		0,
		"no bodies should be holding connections open",
	);
});

test("multiple requests track bodies independently", async (t) => {
	t.plan(4);

	const agent = new Agent();

	const response1 = await fetch(url("/get"), { agent });
	const response2 = await fetch(url("/get"), { agent });
	const response3 = await fetch(url("/get"), { agent });

	await response1.text();
	await response2.text();
	await response3.text();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 3, "bodiesStarted should be 3");
	t.equal(stats.bodiesFinished, 3, "bodiesFinished should be 3");
	t.equal(stats.requestsSent, 3, "requestsSent should be 3");
	t.equal(stats.responsesReceived, 3, "responsesReceived should be 3");
});

test("clone and read from both clones only starts one body", async (t) => {
	t.plan(4);

	const agent = new Agent();

	const response1 = await fetch(url("/get"), { agent });
	const response2 = response1.clone();

	const text1 = await response1.text();
	const text2 = await response2.text();

	t.ok(text1, "first clone should read text");
	t.ok(text2, "second clone should read text");

	const stats = agent.stats();
	t.equal(
		stats.bodiesStarted,
		1,
		"bodiesStarted should be 1 (shared stream)",
	);
	t.equal(
		stats.bodiesFinished,
		1,
		"bodiesFinished should be 1 (shared stream)",
	);
});

test("clone and read from one clone finishes the body", async (t) => {
	t.plan(3);

	const agent = new Agent();

	const response1 = await fetch(url("/get"), { agent });
	const response2 = response1.clone();

	await response1.text();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "bodiesStarted should be 1");
	t.equal(
		stats.bodiesFinished,
		1,
		"bodiesFinished should be 1 (first clone consumed entire stream)",
	);

	const text2 = await response2.text();
	t.ok(text2, "second clone should still be able to read from cached stream");
});

test("partial stream read does not finish body", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await fetch(url("/bytes/1000"), { agent });
	const stream = response.body;
	const reader = stream.getReader();

	await reader.read();
	reader.releaseLock();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "bodiesStarted should be 1");
	t.equal(
		stats.bodiesFinished,
		0,
		"bodiesFinished should be 0 (stream not fully consumed)",
	);
});

test("HEAD request does not start a body", async (t) => {
	t.plan(3);

	const agent = new Agent();

	const response = await fetch(url("/get"), { agent, method: "HEAD" });
	t.equal(response.body, null, "HEAD response should have null body");

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 0, "bodiesStarted should be 0 for HEAD");
	t.equal(stats.bodiesFinished, 0, "bodiesFinished should be 0 for HEAD");
});

test("204 No Content response does not start a body", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const response = await fetch(url("/status/204"), { agent });
	await response.text();

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 0, "bodiesStarted should be 0 for 204");
	t.equal(stats.bodiesFinished, 0, "bodiesFinished should be 0 for 204");
});

test("parallel requests with full consumption balance bodies", async (t) => {
	t.plan(2);

	const agent = new Agent();

	const responses = await Promise.all([
		fetch(url("/get"), { agent }),
		fetch(url("/get"), { agent }),
		fetch(url("/get"), { agent }),
		fetch(url("/get"), { agent }),
		fetch(url("/get"), { agent }),
	]);

	await Promise.all(responses.map((r) => r.text()));

	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 5, "bodiesStarted should be 5");
	t.equal(stats.bodiesFinished, 5, "bodiesFinished should be 5");
});

test("different agents track bodies separately", async (t) => {
	t.plan(4);

	const agent1 = new Agent();
	const agent2 = new Agent();

	const response1 = await fetch(url("/get"), { agent: agent1 });
	const response2 = await fetch(url("/get"), { agent: agent2 });
	const response3 = await fetch(url("/get"), { agent: agent1 });

	await response1.text();
	await response2.text();
	await response3.text();

	const stats1 = agent1.stats();
	const stats2 = agent2.stats();

	t.equal(stats1.bodiesStarted, 2, "agent1 bodiesStarted should be 2");
	t.equal(stats1.bodiesFinished, 2, "agent1 bodiesFinished should be 2");
	t.equal(stats2.bodiesStarted, 1, "agent2 bodiesStarted should be 1");
	t.equal(stats2.bodiesFinished, 1, "agent2 bodiesFinished should be 1");
});

test("response not read does not affect body stats", async (t) => {
	t.plan(2);

	const agent = new Agent();

	await fetch(url("/get"), { agent });

	const stats = agent.stats();
	t.equal(
		stats.bodiesStarted,
		0,
		"bodiesStarted should be 0 when body not accessed",
	);
	t.equal(
		stats.bodiesFinished,
		0,
		"bodiesFinished should be 0 when body not accessed",
	);
});

test("accessing body then consuming via text() balances", async (t) => {
	t.plan(3);

	const agent = new Agent();

	const response1 = await fetch(url("/get"), { agent });
	const response2 = response1.clone();

	response1.body;

	const statsAfterBodyAccess = agent.stats();
	t.equal(
		statsAfterBodyAccess.bodiesStarted,
		1,
		"bodiesStarted should be 1 after body access",
	);

	await response2.text();

	const statsAfterText = agent.stats();
	t.equal(
		statsAfterText.bodiesStarted,
		1,
		"bodiesStarted should still be 1 (shared stream)",
	);
	t.equal(
		statsAfterText.bodiesFinished,
		1,
		"bodiesFinished should be 1 after text() consumes stream",
	);
});

test("stats reflect in-flight bodies", async (t) => {
	t.plan(6);

	const agent = new Agent();

	const response1 = await fetch(url("/get"), { agent });
	const response2 = await fetch(url("/get"), { agent });

	const stream1 = response1.body;
	const stats1 = agent.stats();
	t.equal(stats1.bodiesStarted, 1, "one body started");
	t.equal(
		stats1.bodiesStarted - stats1.bodiesFinished,
		1,
		"one body in flight",
	);

	response2.body;
	const stats2 = agent.stats();
	t.equal(stats2.bodiesStarted, 2, "two bodies started");
	t.equal(
		stats2.bodiesStarted - stats2.bodiesFinished,
		2,
		"two bodies in flight",
	);

	const reader1 = stream1.getReader();
	while (true) {
		const { done } = await reader1.read();
		if (done) break;
	}
	reader1.releaseLock();

	const stats3 = agent.stats();
	t.equal(
		stats3.bodiesFinished,
		1,
		"one body finished after consuming response1",
	);
	t.equal(
		stats3.bodiesStarted - stats3.bodiesFinished,
		1,
		"one body still in flight",
	);
});
