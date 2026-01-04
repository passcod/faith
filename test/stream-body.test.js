const { url } = require("./helpers.js");
const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");

test("Streaming body with ReadableStream", async (t) => {
	t.plan(3);

	const chunks = [
		new TextEncoder().encode("Hello, "),
		new TextEncoder().encode("streaming "),
		new TextEncoder().encode("world!"),
	];

	let index = 0;
	const stream = new ReadableStream({
		pull(controller) {
			if (index < chunks.length) {
				controller.enqueue(chunks[index]);
				index++;
			} else {
				controller.close();
			}
		},
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: stream,
		duplex: "half",
		headers: {
			"Content-Type": "text/plain",
		},
	});

	t.equal(response.status, 200, "should return 200");

	const json = await response.json();
	t.equal(
		json.data,
		"Hello, streaming world!",
		"server should receive complete body",
	);
	t.equal(index, chunks.length, "all chunks should be consumed");
});

test("Streaming body requires duplex option", async (t) => {
	t.plan(2);

	const stream = new ReadableStream({
		start(controller) {
			controller.enqueue(new TextEncoder().encode("test"));
			controller.close();
		},
	});

	try {
		await faithFetch(url("/post"), {
			method: "POST",
			body: stream,
		});
		t.fail("should throw TypeError");
	} catch (err) {
		t.ok(err instanceof TypeError, "should throw TypeError");
		t.ok(
			err.message.includes("duplex"),
			"error message should mention duplex option",
		);
	}
});

test("Streaming body with large payload", async (t) => {
	t.plan(2);

	const chunkSize = 1024;
	const numChunks = 100;
	let chunksProduced = 0;

	const stream = new ReadableStream({
		pull(controller) {
			if (chunksProduced < numChunks) {
				const chunk = new Uint8Array(chunkSize).fill(0x41);
				controller.enqueue(chunk);
				chunksProduced++;
			} else {
				controller.close();
			}
		},
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: stream,
		duplex: "half",
		headers: {
			"Content-Type": "application/octet-stream",
		},
	});

	t.equal(response.status, 200, "should return 200");
	t.equal(chunksProduced, numChunks, "all chunks should be produced");
});

test("Streaming body with abort signal", async (t) => {
	t.plan(2);

	const controller = new AbortController();
	let chunksProduced = 0;

	const stream = new ReadableStream({
		pull(ctrl) {
			if (chunksProduced < 1000) {
				const chunk = new Uint8Array(1024).fill(0x41);
				ctrl.enqueue(chunk);
				chunksProduced++;

				if (chunksProduced === 5) {
					controller.abort();
				}
			} else {
				ctrl.close();
			}
		},
	});

	try {
		await faithFetch(url("/post"), {
			method: "POST",
			body: stream,
			duplex: "half",
			signal: controller.signal,
		});
		t.fail("should throw AbortError");
	} catch (err) {
		t.equal(err.name, "AbortError", "should throw AbortError");
		t.ok(
			chunksProduced >= 5,
			"should have produced at least 5 chunks before abort",
		);
	}
});

test("Streaming body with empty stream", async (t) => {
	t.plan(2);

	const stream = new ReadableStream({
		start(controller) {
			controller.close();
		},
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: stream,
		duplex: "half",
	});

	t.equal(response.status, 200, "should return 200");

	const json = await response.json();
	t.equal(json.data, "", "server should receive empty body");
});

test("Streaming body with async chunks", async (t) => {
	t.plan(2);

	const messages = ["async ", "streaming ", "test"];
	let index = 0;

	const stream = new ReadableStream({
		async pull(controller) {
			if (index < messages.length) {
				await new Promise((resolve) => setTimeout(resolve, 10));
				controller.enqueue(new TextEncoder().encode(messages[index]));
				index++;
			} else {
				controller.close();
			}
		},
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: stream,
		duplex: "half",
		headers: {
			"Content-Type": "text/plain",
		},
	});

	t.equal(response.status, 200, "should return 200");

	const json = await response.json();
	t.equal(
		json.data,
		"async streaming test",
		"server should receive complete body",
	);
});

test("Streaming body with binary data", async (t) => {
	t.plan(2);

	const binaryData = new Uint8Array([0x00, 0x01, 0x02, 0xff, 0xfe, 0xfd]);

	const stream = new ReadableStream({
		start(controller) {
			controller.enqueue(binaryData);
			controller.close();
		},
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: stream,
		duplex: "half",
		headers: {
			"Content-Type": "application/octet-stream",
		},
	});

	t.equal(response.status, 200, "should return 200");

	const json = await response.json();
	t.ok(json.data !== undefined, "server should receive body data");
});

test("Streaming body preserves headers", async (t) => {
	t.plan(3);

	const stream = new ReadableStream({
		start(controller) {
			controller.enqueue(new TextEncoder().encode("test data"));
			controller.close();
		},
	});

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: stream,
		duplex: "half",
		headers: {
			"Content-Type": "text/plain",
			"X-Custom-Header": "custom-value",
		},
	});

	t.equal(response.status, 200, "should return 200");

	const json = await response.json();
	t.equal(
		json.headers["Content-Type"][0],
		"text/plain",
		"Content-Type header should be preserved",
	);
	t.equal(
		json.headers["X-Custom-Header"][0],
		"custom-value",
		"custom header should be preserved",
	);
});

test("Non-streaming body still works", async (t) => {
	t.plan(2);

	const response = await faithFetch(url("/post"), {
		method: "POST",
		body: "regular string body",
		headers: {
			"Content-Type": "text/plain",
		},
	});

	t.equal(response.status, 200, "should return 200");

	const json = await response.json();
	t.equal(
		json.data,
		"regular string body",
		"server should receive string body",
	);
});
