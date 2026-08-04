/**
 * The controllable origin: two Node listeners over the test CA, sharing route
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
const { handle } = require("./controllable-routes.js");

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

const controllableH1 = {
	name: "controllable-h1",
	capabilities: new Set([
		C.H1,
		C.TLS,
		C.TRAILERS,
		C.GZIP,
		C.CHUNKED,
		C.CONTENT_LENGTH,
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
			},
			handle,
		);
		track(server, sockets);
		await new Promise((resolve, reject) => {
			server.once("error", reject);
			server.listen(port, "127.0.0.1", resolve);
		});
		return {
			url: `https://localhost:${port}`,
			ca,
			close: makeCloser(server, sockets),
		};
	},
};

const controllableH2 = {
	name: "controllable-h2",
	capabilities: new Set([
		C.H2,
		C.TLS,
		C.TRAILERS,
		C.GZIP,
		C.CONTENT_LENGTH,
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
		track(server, sockets);
		await new Promise((resolve, reject) => {
			server.once("error", reject);
			server.listen(port, "127.0.0.1", resolve);
		});
		return {
			url: `https://localhost:${port}`,
			ca,
			close: makeCloser(server, sockets),
		};
	},
};

module.exports = { controllableH1, controllableH2 };
