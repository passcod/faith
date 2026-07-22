const test = require("tape");
const { fetch: faithFetch, Agent, ERROR_CODES } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Agent.close() releases the agent and blocks further requests", async (t) => {
	const agent = new Agent();

	// Works before closing.
	const before = await faithFetch(url("/get"), { agent });
	t.equal(before.status, 200, "request before close() succeeds");
	await before.text();

	// Close it.
	agent.close();

	// Any new request now throws a `Closed` error.
	try {
		await faithFetch(url("/get"), { agent });
		t.fail("request after close() should throw");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.Closed, "error code is Closed");
		t.ok(err instanceof TypeError, "Closed maps to a TypeError");
	}

	t.end();
});

test("Agent.close() is idempotent", (t) => {
	const agent = new Agent();
	agent.close();
	t.doesNotThrow(() => agent.close(), "second close() is a no-op");
	t.end();
});

test("Agent.close() leaves the cookie store readable", async (t) => {
	const agent = new Agent({ cookies: true });
	agent.addCookie("https://example.test/", "a=1");
	agent.close();
	t.equal(
		agent.getCookie("https://example.test/"),
		"a=1",
		"getCookie still works after close()",
	);
	t.end();
});

test("in-flight requests complete after close()", async (t) => {
	const agent = new Agent();
	// Start a request, then close before it resolves; it should still complete.
	const inflight = faithFetch(url("/delay/1"), { agent });
	agent.close();
	const res = await inflight;
	t.equal(res.status, 200, "in-flight request finished despite close()");
	await res.text();
	t.end();
});
