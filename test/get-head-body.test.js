const { url } = require("./helpers.js");
const test = require("tape");
const { fetch } = require("../wrapper.js");
const { ReadableStream } = require("stream/web");

// spec:REQ#body — a GET or HEAD request cannot carry a body. Faith refuses a non-null body on
// either method with a TypeError before anything is sent, matching the fetch standard's Request
// constructor.

async function refuses(t, label, options) {
	try {
		await fetch(url("/anything"), options);
		t.fail(`${label}: should have thrown TypeError`);
	} catch (error) {
		t.ok(error instanceof TypeError, `${label}: throws TypeError`);
	}
}

test("GET with a body is refused", async (t) => {
	t.plan(1);
	await refuses(t, "GET + string", { method: "GET", body: "x" });
});

test("HEAD with a body is refused", async (t) => {
	t.plan(1);
	await refuses(t, "HEAD + string", { method: "HEAD", body: "x" });
});

test("a body with no method (defaults to GET) is refused", async (t) => {
	t.plan(1);
	await refuses(t, "default GET + string", { body: "x" });
});

test("a lower-case get with a body is refused", async (t) => {
	t.plan(2);
	await refuses(t, "get + string", { method: "get", body: "x" });
	await refuses(t, "hEaD + string", { method: "hEaD", body: "x" });
});

test("an empty-string body still counts as a body", async (t) => {
	t.plan(1);
	await refuses(t, "GET + empty string", { method: "GET", body: "" });
});

test("a streaming body on GET is refused without locking the stream", async (t) => {
	t.plan(1);
	const stream = new ReadableStream({
		start(controller) {
			controller.enqueue(new TextEncoder().encode("x"));
			controller.close();
		},
	});
	try {
		await fetch(url("/anything"), {
			method: "GET",
			body: stream,
			duplex: "half",
		});
		t.fail("should have thrown TypeError");
	} catch (error) {
		// Refused before the native pump takes a reader, so the stream is never locked.
		t.ok(
			error instanceof TypeError && !stream.locked,
			"throws TypeError without locking the stream",
		);
	}
});

test("GET with a null body is allowed", async (t) => {
	t.plan(1);
	const response = await fetch(url("/get"), { method: "GET", body: null });
	t.equal(response.status, 200, "GET with null body succeeds");
	// These tests assert on the status alone, so the body is discarded rather than left
	// unread holding its connection open.
	await response.discard();
});

test("POST with a body is allowed", async (t) => {
	t.plan(1);
	const response = await fetch(url("/post"), { method: "POST", body: "x" });
	t.equal(response.status, 200, "POST with a body succeeds");
	await response.discard();
});
