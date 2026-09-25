/**
 * Giving up a response body over HTTP/2 resets its stream and leaves the connection in the pool
 * (spec: BODY#giving-up-the-body, CANCEL#what-ending-early-does-on-the-wire).
 *
 * HTTP/2 over TLS, trusting the shared test CA through `tls.extraRoots`. The origin serves a
 * body that never ends, and records how each stream closed and whether the session did.
 */

const test = require("tape");
const http2 = require("node:http2");
const { readFileSync } = require("node:fs");

const { fetch, Agent } = require("../wrapper.js");
const { ensureCert } = require("./fixtures/net.js");

async function origin() {
	const { ca, certPath, keyPath } = ensureCert();
	const sessions = new Set();
	const resets = new Map();
	const waiters = new Map();
	let sessionsClosed = 0;

	const server = http2.createSecureServer({
		key: readFileSync(keyPath),
		cert: readFileSync(certPath),
	});
	server.on("session", (session) => {
		sessions.add(session);
		session.on("close", () => {
			sessionsClosed += 1;
			sessions.delete(session);
		});
	});
	server.on("stream", (stream, headers) => {
		const path = headers[":path"];
		stream.respond({ ":status": 200, "content-type": "application/octet-stream" });
		const timer = setInterval(() => {
			if (!stream.destroyed) stream.write(Buffer.alloc(1024, 1));
		}, 5);
		stream.on("close", () => {
			clearInterval(timer);
			resets.set(path, stream.rstCode);
			waiters.get(path)?.(stream.rstCode);
		});
	});

	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});

	return {
		ca,
		url: (path) => `https://127.0.0.1:${server.address().port}${path}`,
		/** The RST_STREAM code the stream for `path` closed with, or `null` if still open. */
		reset: (path, ms = 1000) =>
			resets.has(path)
				? Promise.resolve(resets.get(path))
				: Promise.race([
						new Promise((resolve) => waiters.set(path, resolve)),
						new Promise((resolve) => setTimeout(() => resolve(null), ms)),
					]),
		sessionsClosed: () => sessionsClosed,
		close: () => {
			for (const session of sessions) session.destroy();
			return new Promise((resolve) => server.close(resolve));
		},
	};
}

test("h2 body cancel: reader.cancel() resets the stream and keeps the connection", async (t) => {
	const server = await origin();
	t.teardown(server.close);
	const agent = new Agent({ tls: { extraRoots: [server.ca] } });

	const res = await fetch(server.url("/one"), { agent });
	t.equal(res.version, "HTTP/2.0", "the response is HTTP/2");
	const reader = res.body.getReader();
	await reader.read();
	await reader.cancel();

	const code = await server.reset("/one");
	t.notEqual(code, null, "the stream closes");
	t.notEqual(code, http2.constants.NGHTTP2_NO_ERROR, "by a reset rather than an ordinary end");

	const next = await fetch(server.url("/two"), { agent });
	// The timing settles once the body ends, which discarding it does.
	await next.discard();
	t.equal((await next.timing).reused, true, "the next request reuses the connection");
	t.equal(server.sessionsClosed(), 0, "the session stays up throughout");
});

test("h2 signal: aborting after headers resets the stream", async (t) => {
	const server = await origin();
	t.teardown(server.close);
	const agent = new Agent({ tls: { extraRoots: [server.ca] } });

	const controller = new AbortController();
	const res = await fetch(server.url("/abort"), { agent, signal: controller.signal });
	const reader = res.body.getReader();
	await reader.read();
	controller.abort();

	try {
		while (!(await reader.read()).done);
		t.fail("the stream should error");
	} catch (error) {
		t.equal(error.name, "AbortError", "the stream errors with the abort");
	}
	const code = await server.reset("/abort");
	t.notEqual(code, null, "the stream closes");
	t.notEqual(code, http2.constants.NGHTTP2_NO_ERROR, "by a reset");
	t.equal(server.sessionsClosed(), 0, "the session stays up");
});
