/**
 * The Node origin: two listeners over the test CA, sharing route
 * handlers.
 *
 * Split into HTTP/1-only and HTTP/2-only rows deliberately. A single
 * `http2.createSecureServer({ allowHTTP1: true })` would advertise h2 in ALPN
 * and faith would always choose it, so the chunked-framing routes would never
 * run. Splitting also keeps the capability declarations honest: HTTP/2 has no
 * chunked transfer encoding, so the h2 row must not claim CHUNKED.
 */

const { readFileSync } = require("node:fs");
const http2 = require("node:http2");
const https = require("node:https");

const { ensureCert, findFreePort } = require("../../fixtures/net.js");
const { CAPABILITIES: C } = require("../capabilities.js");
const { handle, state } = require("./node-routes.js");

/** Destroy live sockets rather than waiting on them. */
function makeCloser(server, sockets) {
	return () =>
		new Promise((resolve) => {
			// The agent under test keeps connections pooled, so a bare
			// server.close() never settles and the caller hangs in its teardown.
			for (const socket of sockets) socket.destroy();
			sockets.clear();
			server.close(resolve);
			setTimeout(resolve, 500).unref();
		});
}

/**
 * Listen, then hand the `error` event over to a logger.
 *
 * The reject listener has to go once startup succeeds: leaving it means a later
 * server error resolves nothing -- rejecting an already-settled promise is a
 * no-op -- and, worse, suppresses Node's own unhandled-`error` crash, so an
 * origin failure becomes invisible and surfaces only as a puzzling client-side
 * symptom. The row name goes in the message so it is obvious which listener died.
 */
async function listen(server, port, name) {
	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(port, "127.0.0.1", resolve);
	});
	server.removeAllListeners("error");
	server.on("error", (err) => {
		console.error(`${name} Node origin error:`, err);
	});
}

function track(server, sockets) {
	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
	server.on("secureConnection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});
}

/**
 * Node enforces both of these itself, so the scriptable origin covers the two
 * connection-lifecycle dimensions rather than leaving them to one Apache row each.
 *
 * `maxRequestsPerSocket` is the HTTP/1 keepalive limit -- Node answers the request
 * that hits it with `Connection: close` -- and `maxHeaderSize` is a whole-header-block
 * ceiling, comfortably above the couple of hundred bytes faith sends by default.
 */
const KEEPALIVE_LIMIT = 2;
const HEADER_LIMIT = 1024;

const nodeH1 = {
	name: "node-h1",
	keepaliveLimit: KEEPALIVE_LIMIT,
	headerLimit: HEADER_LIMIT,
	// The protocol every cell on this row is expected to negotiate. Asserted per
	// cell by the runner, because nothing in the dimensions is version-specific:
	// without it, an h2 row that fell back to HTTP/1.1 would still pass.
	expectVersion: "HTTP/1.1",
	capabilities: new Set([
		C.H1,
		C.TLS,
		C.TRAILERS,
		C.GZIP,
		C.CHUNKED,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		C.KEEPALIVE_LIMIT,
		C.HEADER_LIMITS,
		C.SCRIPTABLE,
	]),
	async start() {
		const { ca, certPath, keyPath } = ensureCert();
		const port = await findFreePort();
		const sockets = new Set();
		const server = https.createServer(
			{
				key: readFileSync(keyPath),
				cert: readFileSync(certPath),
				ALPNProtocols: ["http/1.1"],
				maxHeaderSize: HEADER_LIMIT,
			},
			handle,
		);
		// Node closes the connection on the request that reaches this count, which is
		// what the connection-reuse dimension asks the client to survive.
		server.maxRequestsPerSocket = KEEPALIVE_LIMIT;
		track(server, sockets);
		await listen(server, port, this.name);
		return {
			url: `https://localhost:${port}`,
			ca,
			close: makeCloser(server, sockets),
		};
	},
};

const nodeH2 = {
	name: "node-h2",
	expectVersion: "HTTP/2.0",
	capabilities: new Set([
		C.H2,
		C.TLS,
		C.TRAILERS,
		C.GZIP,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		// Node's h2 server can be told exactly when to send a GOAWAY, which is why the
		// h2-GOAWAY dimension lives on this row rather than on a configured server.
		C.GOAWAY,
		C.SCRIPTABLE,
	]),
	async start() {
		const { ca, certPath, keyPath } = ensureCert();
		const port = await findFreePort();
		const sockets = new Set();
		const server = http2.createSecureServer(
			{
				key: readFileSync(keyPath),
				cert: readFileSync(certPath),
				allowHTTP1: false,
			},
			handle,
		);
		// Counted so the h2-GOAWAY dimension can see that a client opened a fresh session
		// rather than reusing the one the server retired.
		server.on("session", () => {
			state.sessions++;
		});
		track(server, sockets);
		await listen(server, port, this.name);
		return {
			url: `https://localhost:${port}`,
			ca,
			close: makeCloser(server, sockets),
		};
	},
};

module.exports = { nodeH1, nodeH2 };
