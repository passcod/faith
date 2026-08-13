/**
 * Flow-control windows reach the wire (spec:FLOW).
 *
 * An HTTP/2 server sees the client's windows directly, so these assert on what
 * the origin is actually told rather than on the option plumbing: the per-stream
 * window arrives in the client's SETTINGS as `initialWindowSize`, and the
 * whole-connection window is the session's `remoteWindowSize`, being how much the
 * server may send before the client must acknowledge.
 *
 * HTTP/2 over TLS deliberately: h2c would need prior knowledge, and ALPN is how
 * every real HTTP/2 connection is reached. The shared test CA is trusted via
 * `tls.extraRoots` rather than disabling verification.
 */

const test = require("tape");
const http2 = require("node:http2");
const { readFileSync } = require("node:fs");

const { Agent } = require("../index.js");
const { fetch } = require("../wrapper.js");
const { ensureCert } = require("./fixtures/net.js");

const MIB = 1024 * 1024;

/**
 * An origin that reports the flow-control windows the client advertised.
 *
 * The connection window is read at the point the request arrives, by which time
 * the client's post-handshake WINDOW_UPDATE on stream 0 has landed. Node counts
 * the connection window down as it sends, so it is read before the response body
 * is written rather than after.
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
	server.on("stream", (stream) => {
		const observed = {
			streamWindow: stream.session.remoteSettings.initialWindowSize,
			connectionWindow: stream.session.state.remoteWindowSize,
		};
		stream.respond({ ":status": 200, "content-type": "application/json" });
		stream.end(JSON.stringify(observed));
	});

	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});

	return {
		url: `https://127.0.0.1:${server.address().port}`,
		ca,
		close: () =>
			new Promise((resolve) => {
				for (const socket of sockets) socket.destroy();
				sockets.clear();
				server.close(resolve);
				setTimeout(resolve, 500).unref();
			}),
	};
}

/** Fetch once through an agent built with `options`, and report what the origin saw. */
async function windowsSeenBy(server, options) {
	const agent = new Agent({
		...options,
		tls: { extraRoots: [server.ca] },
	});
	try {
		const res = await fetch(server.url, { agent, timeout: 10000 });
		return { version: res.version, ...(await res.json()) };
	} finally {
		agent.close();
	}
}

test("flow control: the defaults reach the origin", async (t) => {
	const server = await origin();
	try {
		const seen = await windowsSeenBy(server, {});
		t.equal(seen.version, "HTTP/2.0", "negotiated h2");
		t.equal(seen.streamWindow, 6 * MIB, "per-stream window is 6 MiB");
		t.equal(seen.connectionWindow, 15 * MIB, "whole-connection window is 15 MiB");
	} finally {
		await server.close();
		t.end();
	}
});

test("flow control: the common windows apply to HTTP/2", async (t) => {
	const server = await origin();
	try {
		const seen = await windowsSeenBy(server, {
			flowControl: { streamWindow: 3 * MIB, connectionWindow: 9 * MIB },
		});
		t.equal(seen.streamWindow, 3 * MIB, "flowControl.streamWindow applied");
		t.equal(seen.connectionWindow, 9 * MIB, "flowControl.connectionWindow applied");
	} finally {
		await server.close();
		t.end();
	}
});

test("flow control: an http2 window beats the common one", async (t) => {
	const server = await origin();
	try {
		const seen = await windowsSeenBy(server, {
			flowControl: { streamWindow: 3 * MIB, connectionWindow: 9 * MIB },
			http2: { streamWindow: 5 * MIB },
		});
		t.equal(seen.streamWindow, 5 * MIB, "http2.streamWindow wins");
		t.equal(
			seen.connectionWindow,
			9 * MIB,
			"the connection window falls back to the common one on its own",
		);
	} finally {
		await server.close();
		t.end();
	}
});

test("flow control: an http2 window applies without the common group", async (t) => {
	const server = await origin();
	try {
		const seen = await windowsSeenBy(server, {
			http2: { streamWindow: 4 * MIB, connectionWindow: 12 * MIB },
		});
		t.equal(seen.streamWindow, 4 * MIB, "http2.streamWindow applied");
		t.equal(seen.connectionWindow, 12 * MIB, "http2.connectionWindow applied");
	} finally {
		await server.close();
		t.end();
	}
});

test("flow control: adaptive windowing takes over both windows", async (t) => {
	const server = await origin();
	try {
		// Adaptive windowing owns both windows, so the explicit sizes set alongside it
		// are ignored and the connection opens at h2's own default instead: the ramp
		// starts from there and grows towards a measured estimate.
		const seen = await windowsSeenBy(server, {
			flowControl: { streamWindow: 3 * MIB, connectionWindow: 9 * MIB },
			http2: { adaptiveWindow: true, streamWindow: 5 * MIB },
		});
		t.notEqual(seen.streamWindow, 5 * MIB, "http2.streamWindow does not apply");
		t.notEqual(seen.streamWindow, 3 * MIB, "flowControl.streamWindow does not apply");
		t.ok(
			seen.streamWindow < MIB,
			`opens far below the static default, at ${seen.streamWindow} bytes`,
		);
	} finally {
		await server.close();
		t.end();
	}
});
