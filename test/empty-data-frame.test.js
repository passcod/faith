/**
 * An origin that ends a response with an empty DATA frame carrying END_STREAM
 * (observed behind some HTTP/2 edges) must not break body streaming.
 *
 * A zero-length chunk carries no bytes, but the body's byte-oriented
 * ReadableStream cannot take one: `ReadableByteStreamController.enqueue` rejects
 * an empty buffer with `ERR_INVALID_STATE`. So the empty trailing frame has to be
 * dropped before it reaches the stream, and every consumer -- `getReader()`,
 * `Readable.fromWeb()`, and the collecting `text()` -- has to read the full body
 * and finish cleanly.
 *
 * HTTP/2 over TLS deliberately: an empty interior DATA frame has no equivalent in
 * HTTP/1.1 chunked framing, where a zero-length chunk is the terminator. The shared
 * test CA is trusted via `tls.extraRoots` rather than disabling verification.
 */

const test = require("tape");
const http2 = require("node:http2");
const { Readable } = require("node:stream");
const { readFileSync } = require("node:fs");

const { Agent } = require("../index.js");
const { fetch } = require("../wrapper.js");
const { ensureCert } = require("./fixtures/net.js");

const PAYLOAD = "empty-frame-payload";

/**
 * `/one` writes the payload in a single frame; `/many` splits it across several.
 * Both flush their data and then `end()` on a later tick, so END_STREAM rides an
 * empty trailing DATA frame rather than the last payload frame.
 */
async function origin() {
	const { ca, certPath, keyPath } = ensureCert();
	const sockets = new Set();
	const server = http2.createSecureServer({
		key: readFileSync(keyPath),
		cert: readFileSync(certPath),
	});
	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
	server.on("stream", (stream, headers) => {
		stream.respond({ ":status": 200, "content-type": "text/plain" });
		if (headers[":path"] === "/many") {
			for (const part of ["empty-", "frame-", "payload"]) stream.write(part);
		} else {
			stream.write(PAYLOAD);
		}
		setImmediate(() => stream.end());
	});

	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});

	return {
		url: `https://127.0.0.1:${server.address().port}`,
		agent: new Agent({ tls: { extraRoots: [ca] } }),
		close: () =>
			new Promise((resolve) => {
				for (const socket of sockets) socket.destroy();
				sockets.clear();
				server.close(resolve);
				setTimeout(resolve, 500).unref();
			}),
	};
}

async function viaReader(res) {
	const reader = res.body.getReader();
	let n = 0;
	for (;;) {
		const { done, value } = await reader.read();
		if (done) break;
		n += value.length;
	}
	return n;
}

async function viaFromWeb(res) {
	let n = 0;
	for await (const chunk of Readable.fromWeb(res.body)) n += chunk.length;
	return n;
}

test("empty trailing DATA frame: getReader() reads the body and closes", async (t) => {
	const server = await origin();
	try {
		const res = await fetch(`${server.url}/one`, { agent: server.agent, timeout: 10000 });
		t.equal(res.version, "HTTP/2.0", "negotiated h2");
		t.equal(await viaReader(res), PAYLOAD.length, "all bytes delivered, stream closed");
	} finally {
		await server.close();
		t.end();
	}
});

test("empty trailing DATA frame: Readable.fromWeb() reads the body and closes", async (t) => {
	const server = await origin();
	try {
		const res = await fetch(`${server.url}/one`, { agent: server.agent, timeout: 10000 });
		t.equal(await viaFromWeb(res), PAYLOAD.length, "all bytes delivered, stream closed");
	} finally {
		await server.close();
		t.end();
	}
});

test("empty trailing DATA frame: a multi-chunk body streams intact", async (t) => {
	const server = await origin();
	try {
		const res = await fetch(`${server.url}/many`, { agent: server.agent, timeout: 10000 });
		t.equal(await viaReader(res), PAYLOAD.length, "all chunks delivered, stream closed");
	} finally {
		await server.close();
		t.end();
	}
});

test("empty trailing DATA frame: text() still reads the whole body", async (t) => {
	const server = await origin();
	try {
		const res = await fetch(`${server.url}/one`, { agent: server.agent, timeout: 10000 });
		t.equal(await res.text(), PAYLOAD, "text() reads the full body");
	} finally {
		await server.close();
		t.end();
	}
});
