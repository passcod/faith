/**
 * Giving up a response body reaches the network (spec: BODY#giving-up-the-body).
 *
 * Each response and clone holds a claim on the body. Cancelling its stream, `discard()`, or
 * garbage collection gives the claim up, and the last claim to go stops the transfer: over
 * HTTP/1 the remainder is read out within the agent's drain limits so the connection can be
 * reused, and past them the connection is closed. A local origin serves bodies that never end,
 * so a connection still open after its body was given up is visible as a socket the origin has
 * not seen close.
 */

const test = require("tape");
const http = require("node:http");
const v8 = require("node:v8");
const vm = require("node:vm");

const { fetch, Agent, ERROR_CODES } = require("../wrapper.js");

v8.setFlagsFromString("--expose-gc");
const gc = vm.runInNewContext("gc");

/**
 * An origin whose paths each serve a different body:
 * - `/endless`: chunked, a small chunk every few milliseconds, forever
 * - `/big`: advertises a large `Content-Length` and trickles towards it
 * - `/small`: a `Content-Length` body sent in two halves, the second a little later
 * - `/stall`: a `Content-Length` body that sends its first half and then nothing
 *
 * `closed(path)` resolves when the socket that served `path` closes.
 */
async function origin() {
	const closes = new Map();
	const sockets = new Set();
	const closeOf = (path) => {
		if (!closes.has(path)) {
			let resolve;
			const promise = new Promise((r) => {
				resolve = r;
			});
			closes.set(path, { promise, resolve });
		}
		return closes.get(path);
	};

	const server = http.createServer((req, res) => {
		const path = req.url;
		req.socket.on("close", () => closeOf(path).resolve(true));
		const trickle = () => {
			const timer = setInterval(() => res.write(Buffer.alloc(1024, 1)), 5);
			req.socket.on("close", () => clearInterval(timer));
		};
		if (path.startsWith("/endless")) {
			res.writeHead(200, { "content-type": "application/octet-stream" });
			trickle();
		} else if (path.startsWith("/big")) {
			res.writeHead(200, { "content-length": String(64 * 1024 * 1024) });
			trickle();
		} else if (path.startsWith("/small")) {
			res.writeHead(200, { "content-length": "20000" });
			res.write(Buffer.alloc(10000, 1));
			setTimeout(() => res.end(Buffer.alloc(10000, 2)), 50);
		} else if (path.startsWith("/stall")) {
			res.writeHead(200, { "content-length": "20000" });
			res.write(Buffer.alloc(10000, 1));
		} else {
			res.writeHead(404);
			res.end();
		}
	});
	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
	await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));

	return {
		url: (path) => `http://127.0.0.1:${server.address().port}${path}`,
		/** Whether the socket that served `path` closes within `ms`. */
		closed: (path, ms = 500) =>
			Promise.race([
				closeOf(path).promise,
				new Promise((resolve) => setTimeout(() => resolve(false), ms)),
			]),
		close: () => {
			for (const socket of sockets) socket.destroy();
			return new Promise((resolve) => server.close(resolve));
		},
	};
}

/** An agent that closes the connection of any abandoned body, so closing is prompt. */
const noDrain = () => new Agent({ pool: { drainLimit: 0 } });

test("body cancel: reader.cancel() closes the connection of an endless HTTP/1 body", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/endless"), { agent: noDrain() });
	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();

	t.ok(await server.closed("/endless"), "the origin sees the connection close");
	t.ok(res, "while the response is still held");
});

test("body cancel: leaving a for await loop early closes the connection", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/endless"), { agent: noDrain() });
	for await (const _ of res.body) break;

	t.ok(await server.closed("/endless"), "the origin sees the connection close");
	t.ok(res, "while the response is still held");
});

test("body cancel: cancelling while a read waits on the network still reaches it", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/stall"), { agent: noDrain() });
	const reader = res.body.getReader();
	await reader.read();
	// The origin sends nothing more, so this read is still waiting when the cancel lands.
	const pending = reader.read();
	await reader.cancel();

	t.deepEqual(await pending, { done: true, value: undefined }, "the waiting read ends");
	t.ok(await server.closed("/stall"), "the origin sees the connection close");
});

test("body cancel: garbage collecting an unread response closes the connection", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	await (async () => {
		const res = await fetch(server.url("/endless"), { agent: noDrain() });
		t.equal(res.status, 200, "the response arrives");
	})();
	// Collection is not immediate after the last reference goes; a few passes let the
	// finaliser run.
	for (let i = 0; i < 5; i++) {
		gc();
		await new Promise((resolve) => setImmediate(resolve));
	}

	t.ok(await server.closed("/endless", 2000), "the origin sees the connection close");
});

test("body cancel: discard() on an endless body settles and closes the connection", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/endless"), {
		agent: new Agent({ pool: { drainTimeout: 100 } }),
	});
	await res.discard();

	t.pass("discard() settles");
	t.ok(await server.closed("/endless"), "the origin sees the connection close");
	t.equal(res.bodyUsed, false, "discarding is not reading");
	try {
		await res.text();
		t.fail("the body cannot be read after discard()");
	} catch (error) {
		t.equal(error.code, ERROR_CODES.ResponseAlreadyDisturbed, "reading after discard() is refused");
	}
});

test("body cancel: a clone still reading keeps the transfer going", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/endless"), { agent: noDrain() });
	const clone = res.clone();

	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();
	t.notOk(await server.closed("/endless", 200), "the connection stays open for the clone");

	const cloneReader = clone.body.getReader();
	const { done, value } = await cloneReader.read();
	t.notOk(done, "the clone reads on");
	t.ok(value.byteLength > 0, "and gets bytes");

	await cloneReader.cancel();
	t.ok(await server.closed("/endless"), "the last claim going closes the connection");
});

test("body cancel: discard() on the original leaves a clone reading the whole body", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/small"), { agent: noDrain() });
	const clone = res.clone();
	await res.discard();

	const bytes = await clone.bytes();
	t.equal(bytes.byteLength, 20000, "the clone reads the whole body");
	t.deepEqual(await clone.trailers, null, "trailers settle once the clone reaches the end");
});

test("body cancel: discard() errors this response's own stream mid-read", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/stall"), { agent: noDrain() });
	const reader = res.body.getReader();
	await reader.read();
	// Settled into a value up front, so the rejection is handled while discard() runs.
	const pending = reader.read().then(
		() => null,
		(error) => error,
	);
	await res.discard();

	const error = await pending;
	t.ok(error, "the waiting read errors");
	t.equal(error?.code, ERROR_CODES.ResponseAlreadyDisturbed, "with the already-disturbed error");
});

test("body cancel: a small remainder is drained and the connection reused", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const agent = new Agent();
	const res = await fetch(server.url("/small"), { agent });
	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();

	t.notOk(await server.closed("/small", 300), "the connection is kept");
	const next = await fetch(server.url("/small2"), { agent });
	await next.bytes();
	t.equal((await next.timing).reused, true, "the next request reuses it");
});

test("body cancel: drainLimit 0 closes the connection even for a small remainder", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/small"), { agent: noDrain() });
	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();

	t.ok(await server.closed("/small"), "the origin sees the connection close");
});

test("body cancel: a Content-Length remainder over the limit closes at once", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	// A drain timeout long enough that only the length check can explain a prompt close.
	const res = await fetch(server.url("/big"), {
		agent: new Agent({ pool: { drainTimeout: 60_000 } }),
	});
	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();

	t.ok(await server.closed("/big", 300), "the origin sees the connection close");
});

test("body cancel: drainTimeout closes a remainder that stalls", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const res = await fetch(server.url("/stall"), {
		agent: new Agent({ pool: { drainTimeout: 100 } }),
	});
	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();

	t.ok(await server.closed("/stall", 1000), "the origin sees the connection close");
});

test("body cancel: a cancelled body settles its trailers, timing, and the finished counter", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const agent = noDrain();
	const res = await fetch(server.url("/endless"), { agent });
	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();

	t.equal(await res.trailers, null, "trailers resolve to null");
	t.ok((await res.timing).responseEnd > 0, "the timing settles");
	const stats = agent.stats();
	t.equal(stats.bodiesStarted, 1, "one body started");
	t.equal(stats.bodiesFinished, 1, "and it counts as finished, not leaked");
});

test("signal: aborting after headers errors the body stream with the signal's reason", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const controller = new AbortController();
	const res = await fetch(server.url("/endless"), {
		agent: noDrain(),
		signal: controller.signal,
	});
	const reader = res.body.getReader();
	await reader.read();

	const reason = new Error("caller gave up");
	controller.abort(reason);

	try {
		// Buffered chunks are dropped with the error rather than delivered first.
		while (!(await reader.read()).done);
		t.fail("the stream should error");
	} catch (error) {
		t.equal(error, reason, "the stream errors with the signal's reason");
	}
	t.ok(await server.closed("/endless"), "the origin sees the connection close");
});

test("signal: aborting after headers errors every clone and rejects whole-body reads", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const controller = new AbortController();
	const res = await fetch(server.url("/endless"), {
		agent: noDrain(),
		signal: controller.signal,
	});
	const clone = res.clone();
	const cloneBody = clone.body.getReader();
	const text = res.text();

	controller.abort();

	try {
		await text;
		t.fail("text() should reject");
	} catch (error) {
		t.equal(error.name, "AbortError", "text() rejects with an AbortError");
		t.equal(error.code, ERROR_CODES.Aborted, "coded Aborted");
	}
	try {
		while (!(await cloneBody.read()).done);
		t.fail("the clone's stream should error");
	} catch (error) {
		t.equal(error.name, "AbortError", "the clone's stream errors with the default reason");
	}
	t.ok(await server.closed("/endless"), "the origin sees the connection close");
});

test("signal: aborting after the body was read changes nothing", async (t) => {
	const server = await origin();
	t.teardown(server.close);

	const controller = new AbortController();
	const res = await fetch(server.url("/small"), { signal: controller.signal });
	const bytes = await res.bytes();
	controller.abort();

	t.equal(bytes.byteLength, 20000, "the body read in full");
	t.equal(await res.trailers, null, "and its trailers settled as usual");
});

test("signal: toFile() under way rejects with Aborted", async (t) => {
	const { mkdtempSync } = require("node:fs");
	const path = require("node:path");
	const os = require("node:os");
	const server = await origin();
	t.teardown(server.close);

	const controller = new AbortController();
	const res = await fetch(server.url("/endless"), {
		agent: noDrain(),
		signal: controller.signal,
	});
	const dir = mkdtempSync(path.join(os.tmpdir(), "faith-abort-"));
	const write = res.toFile(path.join(dir, "out.bin"));
	setTimeout(() => controller.abort(), 50);

	try {
		await write;
		t.fail("the write should reject");
	} catch (error) {
		t.equal(error.code, ERROR_CODES.Aborted, "the write rejects with Aborted");
	}
});
