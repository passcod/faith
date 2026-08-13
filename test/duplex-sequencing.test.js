/**
 * Full duplex: the response is surfaced while the request body is still going out.
 *
 * This is the property the `duplex` option names but does not control — Faith runs
 * full duplex whatever the option says (spec:REQ#duplex). It holds because the stack
 * writes the request body and reads the response concurrently, which is a property of
 * how reqwest and hyper compose rather than something Faith asks for, so it is pinned
 * here: a stack upgrade that took it away would otherwise pass silently.
 *
 * Cleartext HTTP/1.1 deliberately, on an OS-assigned port. The fetch standard rules
 * out a streaming request body on HTTP/1.x altogether, so testing it there covers both
 * the duplex behaviour and the divergence that makes it reachable.
 */

const test = require("tape");
const http = require("node:http");

const { Agent } = require("../index.js");
const { fetch } = require("../wrapper.js");

/** How long a request body is held open while the response is examined. */
const HOLD_MS = 1000;

/** The margin allowed when asserting something happened before the hold elapsed. */
const MARGIN_MS = 250;

/**
 * `/early` answers immediately without waiting for the request body, and finishes the
 * response once the body ends. `/echo` replies to each request chunk as it arrives,
 * which is what makes an interactive exchange observable.
 */
async function origin() {
	const sockets = new Set();
	const server = http.createServer((req, res) => {
		res.writeHead(200, { "content-type": "text/plain" });
		if (req.url === "/echo") {
			req.on("data", (chunk) => res.write(`echo:${chunk.toString().trim()}\n`));
			req.on("end", () => res.end("bye\n"));
			return;
		}
		res.write("early\n");
		req.on("data", () => {});
		req.on("end", () => res.end("done\n"));
	});
	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});

	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});

	return {
		url: `http://127.0.0.1:${server.address().port}`,
		agent: new Agent(),
		close: () =>
			new Promise((resolve) => {
				// The agent pools its connection, so a bare close() never settles.
				for (const socket of sockets) socket.destroy();
				sockets.clear();
				server.close(resolve);
				setTimeout(resolve, 500).unref();
			}),
	};
}

/** A request body that stays open until the caller closes it. */
function heldBody(first = "chunk") {
	let controller;
	const stream = new ReadableStream({
		start(c) {
			controller = c;
			c.enqueue(new TextEncoder().encode(first));
		},
	});
	return {
		stream,
		push: (text) => controller.enqueue(new TextEncoder().encode(text)),
		close: () => controller.close(),
	};
}

test("duplex: the response arrives while the request body is still open", async (t) => {
	const server = await origin();
	const body = heldBody();
	const started = Date.now();
	const closeAt = setTimeout(() => body.close(), HOLD_MS);

	try {
		const res = await fetch(`${server.url}/early`, {
			method: "POST",
			body: body.stream,
			duplex: "half",
			agent: server.agent,
			timeout: 10000,
		});
		const elapsed = Date.now() - started;

		t.equal(res.status, 200, "the response is there");
		t.ok(
			elapsed < HOLD_MS - MARGIN_MS,
			`and arrived at +${elapsed}ms, before the body closed at +${HOLD_MS}ms`,
		);

		body.close();
		t.equal(await res.text(), "early\ndone\n", "the whole body still reads back");
	} finally {
		clearTimeout(closeAt);
		await server.close();
		t.end();
	}
});

test("duplex: the response body can be read while the request body is still open", async (t) => {
	const server = await origin();
	const body = heldBody();
	const started = Date.now();
	const closeAt = setTimeout(() => body.close(), HOLD_MS);

	try {
		const res = await fetch(`${server.url}/early`, {
			method: "POST",
			body: body.stream,
			duplex: "half",
			agent: server.agent,
			timeout: 10000,
		});

		const reader = res.body.getReader();
		const { value } = await reader.read();
		const elapsed = Date.now() - started;

		t.equal(
			new TextDecoder().decode(value),
			"early\n",
			"the first response chunk reads back",
		);
		t.ok(
			elapsed < HOLD_MS - MARGIN_MS,
			`at +${elapsed}ms, before the body closed at +${HOLD_MS}ms`,
		);

		body.close();
		await reader.cancel();
	} finally {
		clearTimeout(closeAt);
		await server.close();
		t.end();
	}
});

test("duplex: the request body can be driven from the response body", async (t) => {
	const server = await origin();
	const body = heldBody("ping-1\n");

	try {
		const res = await fetch(`${server.url}/echo`, {
			method: "POST",
			body: body.stream,
			duplex: "half",
			agent: server.agent,
			timeout: 10000,
		});

		// Each reply drives the next request chunk. Under half duplex this deadlocks,
		// because the response would not be readable until the body had ended.
		const reader = res.body.getReader();
		const decoder = new TextDecoder();
		const seen = [];
		while (seen.length < 3) {
			const { value, done } = await reader.read();
			if (done) break;
			seen.push(decoder.decode(value).trim());
			if (seen.length < 3) body.push(`ping-${seen.length + 1}\n`);
		}

		t.deepEqual(
			seen,
			["echo:ping-1", "echo:ping-2", "echo:ping-3"],
			"three interactive round-trips completed over one request",
		);

		body.close();
		await reader.cancel();
	} finally {
		await server.close();
		t.end();
	}
});
