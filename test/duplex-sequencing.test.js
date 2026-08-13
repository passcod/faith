/**
 * Full duplex: the response is surfaced while the request body is still going out.
 *
 * This is the property the `duplex` option names but does not control — Faith runs
 * full duplex whatever the option says (spec:REQ#duplex). It holds because the stack
 * writes the request body and reads the response concurrently, which is a property of
 * how reqwest and hyper compose rather than something Faith asks for, so it is pinned
 * here: a stack upgrade that took it away would otherwise pass silently.
 *
 * Both protocols are covered, because they reach full duplex by different routes.
 * HTTP/1.1 gets there only through a streaming request body, which the fetch standard
 * rules out on HTTP/1.x and Faith therefore refuses unless the agent opts in with
 * `quirks.h1RequestStreaming` (spec:QUIRK); HTTP/2 gets there because the transport
 * multiplexes, which is the case the standard would be describing if it defined `full`.
 * A regression could take one without the other, so neither stands in for the other.
 *
 * HTTP/2 over TLS deliberately: h2c would need prior knowledge, and ALPN is how every
 * real HTTP/2 connection is reached. The shared test CA is trusted via `tls.extraRoots`
 * rather than by disabling verification.
 */

const test = require("tape");
const http = require("node:http");
const http2 = require("node:http2");
const { readFileSync } = require("node:fs");

const { Agent } = require("../index.js");
const { fetch } = require("../wrapper.js");
const { ensureCert } = require("./fixtures/net.js");

/** How long a request body is held open while the response is examined. */
const HOLD_MS = 1000;

/** The margin allowed when asserting something happened before the hold elapsed. */
const MARGIN_MS = 250;

/**
 * Both origins answer `/early` immediately without waiting for the request body, and
 * finish the response once the body ends. `/echo` replies to each request chunk as it
 * arrives, which is what makes an interactive exchange observable.
 */
function routes({ writeHead, write, end, onData, onEnd, path }) {
	writeHead();
	if (path === "/echo") {
		onData((chunk) => write(`echo:${chunk.toString().trim()}\n`));
		onEnd(() => end("bye\n"));
		return;
	}
	write("early\n");
	onData(() => {});
	onEnd(() => end("done\n"));
}

/** Track sockets so a pooled connection cannot keep `close()` from settling. */
function track(server) {
	const sockets = new Set();
	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
	return () =>
		new Promise((resolve) => {
			for (const socket of sockets) socket.destroy();
			sockets.clear();
			server.close(resolve);
			setTimeout(resolve, 500).unref();
		});
}

async function listen(server) {
	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});
	return server.address().port;
}

async function h1Origin() {
	const server = http.createServer((req, res) =>
		routes({
			path: req.url,
			writeHead: () => res.writeHead(200, { "content-type": "text/plain" }),
			write: (text) => res.write(text),
			end: (text) => res.end(text),
			onData: (fn) => req.on("data", fn),
			onEnd: (fn) => req.on("end", fn),
		}),
	);
	const close = track(server);
	const port = await listen(server);
	return {
		version: "HTTP/1.1",
		url: `http://127.0.0.1:${port}`,
		// Full duplex over HTTP/1.1 rides on a streaming request body, which is exactly what
		// the standard rules out there, so this origin is only reachable through the quirk.
		agent: new Agent({ quirks: { h1RequestStreaming: true } }),
		close,
	};
}

async function h2Origin() {
	const { ca, certPath, keyPath } = ensureCert();
	const server = http2.createSecureServer({
		key: readFileSync(keyPath),
		cert: readFileSync(certPath),
	});
	const close = track(server);
	server.on("stream", (stream, headers) =>
		routes({
			path: headers[":path"],
			writeHead: () =>
				stream.respond({ ":status": 200, "content-type": "text/plain" }),
			write: (text) => stream.write(text),
			end: (text) => stream.end(text),
			onData: (fn) => stream.on("data", fn),
			onEnd: (fn) => stream.on("end", fn),
		}),
	);
	const port = await listen(server);
	return {
		version: "HTTP/2.0",
		url: `https://127.0.0.1:${port}`,
		agent: new Agent({ tls: { extraRoots: [ca] } }),
		close,
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

for (const [name, makeOrigin] of [
	["http/1.1", h1Origin],
	["http/2", h2Origin],
]) {
	test(`duplex (${name}): the response arrives while the request body is still open`, async (t) => {
		const server = await makeOrigin();
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
			t.equal(res.version, server.version, `over ${server.version}`);
			t.ok(
				elapsed < HOLD_MS - MARGIN_MS,
				`and arrived at +${elapsed}ms, before the body closed at +${HOLD_MS}ms`,
			);

			body.close();
			t.equal(
				await res.text(),
				"early\ndone\n",
				"the whole body still reads back",
			);
		} finally {
			clearTimeout(closeAt);
			server.agent.close();
			await server.close();
			t.end();
		}
	});

	test(`duplex (${name}): the response body can be read while the request body is still open`, async (t) => {
		const server = await makeOrigin();
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

			t.equal(res.version, server.version, `over ${server.version}`);
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
			server.agent.close();
			await server.close();
			t.end();
		}
	});

	test(`duplex (${name}): the request body can be driven from the response body`, async (t) => {
		const server = await makeOrigin();
		const body = heldBody("ping-1\n");

		try {
			const res = await fetch(`${server.url}/echo`, {
				method: "POST",
				body: body.stream,
				duplex: "half",
				agent: server.agent,
				timeout: 10000,
			});
			t.equal(res.version, server.version, `over ${server.version}`);

			// Each reply drives the next request chunk. Under half duplex this
			// deadlocks, because the response would not be readable until the body
			// had ended.
			const reader = res.body.getReader();
			const decoder = new TextDecoder();
			const seen = [];
			while (seen.length < 3) {
				const { value, done } = await reader.read();
				if (done) break;
				for (const line of decoder.decode(value).split("\n")) {
					if (line.trim()) seen.push(line.trim());
				}
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
			server.agent.close();
			await server.close();
			t.end();
		}
	});
}
