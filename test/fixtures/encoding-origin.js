/**
 * An origin that controls content coding exactly, for the cases go-httpbin cannot reach.
 *
 * go-httpbin compresses on its own terms: it has no route that layers two codings, none
 * that sends a coding on a cacheable response, and none that lets a `HEAD` describe a
 * compressed representation. Those are the cases where the decode decision has to be read
 * off the request rather than the response, so the origin has to be ours.
 *
 * Every route picks its coding without consulting `Accept-Encoding`. That is the point: a
 * server is free to compress in the face of `identity`, and Faith has to hand those bytes
 * over as sent rather than decoding them.
 *
 * Cleartext HTTP/1.1 on an OS-assigned port, following `test/trailers.test.js`: coding has
 * nothing to do with TLS, and the shared test CA is a source of failures of its own.
 */

const http = require("node:http");
const zlib = require("node:zlib");

/** The bytes every route sends once decoded. Repetitive, so coding actually shrinks it. */
const PAYLOAD = JSON.stringify({
	message: "the quick brown fox jumps over the lazy dog",
	repeated: "fox ".repeat(64),
});

/** Wire token to encoder. `deflate` is zlib-wrapped (RFC 1950), as clients decode it. */
const ENCODERS = {
	gzip: zlib.gzipSync,
	deflate: zlib.deflateSync,
	br: zlib.brotliCompressSync,
	zstd: zlib.zstdCompressSync,
};

/** Whether this Node build can produce `coding`. `zstdCompressSync` landed in Node 22.15. */
function canEncode(coding) {
	return typeof ENCODERS[coding] === "function";
}

/**
 * Apply codings in `Content-Encoding` order: the header lists them in the order applied,
 * so `gzip, br` means gzip first and brotli over the top of it.
 */
function encode(body, codings) {
	return codings.reduce((bytes, coding) => ENCODERS[coding](bytes), body);
}

/**
 * Routes:
 *
 * - `/coded/:coding` — one coding
 * - `/layered/:a/:b` — both codings applied in order, named on one `Content-Encoding` line
 * - `/layered-lines/:a/:b` — the same two codings, one `Content-Encoding` line each
 * - `/mislabelled/:coding` — uncompressed bytes under a `Content-Encoding` naming `:coding`,
 *   for a coding no encoder here produces (`compress`, say). A client that cannot decode the
 *   named coding must hand the bytes over untouched, which is what makes the body readable.
 * - `/cacheable/:coding` — one coding, `Cache-Control: max-age=60`, `Vary: Accept-Encoding`
 * - `/cacheable-novary/:coding` — the same without `Vary`, so one stored entry answers every
 *   request whatever it advertised
 * - `/sink` — reads the request body and reports it back base64'd, with the request headers,
 *   so a test can read the bytes and the `Content-Encoding` that went out
 * - `/refuses-coding` — answers `415` with an `Accept-Encoding` naming what it would take,
 *   the only thing HTTP gives a caller who guessed wrong about request content
 * - `/redirect-to-sink/:status` — redirects to `/sink` with `:status`, for what a replayed
 *   body carries across a hop
 * - `/echo` — uncompressed, reporting the request headers it saw
 *
 * Every response carries `x-request-count`, so a test can tell a cache hit (the count does
 * not advance) from a trip to the network, and `x-decoded-length` for the decoded size.
 */
function createEncodingOrigin() {
	const sockets = new Set();
	let requests = 0;
	/** Every request the origin saw, newest last, so tests can assert on the wire value. */
	const seen = [];

	const server = http.createServer((req, res) => {
		requests += 1;
		seen.push({
			method: req.method,
			url: req.url,
			acceptEncoding: req.headers["accept-encoding"] ?? null,
		});

		const body = Buffer.from(PAYLOAD, "utf8");
		const parts = req.url.split("?")[0].split("/").filter(Boolean);

		/**
		 * Send `body` under `codings`. `separateLines` sends one `Content-Encoding` line per
		 * coding (Node's array form) rather than the comma-joined equivalent.
		 */
		const send = (codings, { separateLines = false, headers = {} } = {}) => {
			const unsupported = codings.filter((coding) => !canEncode(coding));
			if (unsupported.length) {
				// A Node too old for a coding: say so, rather than lying with a body in a
				// coding the header does not name.
				res.writeHead(501, { "content-type": "text/plain" });
				res.end(`unsupported coding: ${unsupported.join(", ")}`);
				return;
			}

			const encoded = encode(body, codings);
			res.setHeader("content-type", "application/json");
			res.setHeader("x-request-count", String(requests));
			res.setHeader("x-decoded-length", String(body.length));
			for (const [name, value] of Object.entries(headers)) {
				res.setHeader(name, value);
			}
			if (codings.length) {
				res.setHeader("content-encoding", separateLines ? codings : codings.join(", "));
			}
			// Set on HEAD too, where the coding headers describe the representation a GET
			// would return and there are no bytes to decode.
			res.setHeader("content-length", String(encoded.length));

			res.writeHead(200);
			res.end(req.method === "HEAD" ? undefined : encoded);
		};

		switch (parts[0]) {
			case "coded":
				if (parts[1]) return send([parts[1]]);
				break;

			case "layered":
				if (parts[1] && parts[2]) return send([parts[1], parts[2]]);
				break;

			case "layered-lines":
				if (parts[1] && parts[2]) {
					return send([parts[1], parts[2]], { separateLines: true });
				}
				break;

			case "mislabelled":
				if (parts[1]) {
					// No coding applied, but the header claims one. Nothing here can produce
					// `compress`, and that is the point: the label is what is under test.
					res.setHeader("content-type", "application/json");
					res.setHeader("x-request-count", String(requests));
					res.setHeader("x-decoded-length", String(body.length));
					res.setHeader("content-encoding", parts[1]);
					res.setHeader("content-length", String(body.length));
					res.writeHead(200);
					res.end(req.method === "HEAD" ? undefined : body);
					return;
				}
				break;

			case "cacheable":
				if (parts[1]) {
					return send([parts[1]], {
						headers: {
							"cache-control": "max-age=60",
							vary: "Accept-Encoding",
							etag: '"coded"',
						},
					});
				}
				break;

			case "cacheable-novary":
				if (parts[1]) {
					return send([parts[1]], {
						headers: { "cache-control": "max-age=60", etag: '"coded-novary"' },
					});
				}
				break;

			case "sink": {
				// Reports the request as it arrived: the bytes on the wire, base64'd so a
				// compressed body survives the trip back, and the headers describing them.
				// This is the request-compression counterpart to the coded routes above.
				const chunks = [];
				req.on("data", (chunk) => chunks.push(chunk));
				req.on("end", () => {
					const received = Buffer.concat(chunks);
					const payload = JSON.stringify({
						method: req.method,
						headers: req.headers,
						bodyBase64: received.toString("base64"),
						bodyLength: received.length,
					});
					res.writeHead(200, {
						"content-type": "application/json",
						"content-length": String(Buffer.byteLength(payload)),
						"x-request-count": String(requests),
					});
					res.end(req.method === "HEAD" ? undefined : payload);
				});
				return;
			}

			case "refuses-coding": {
				// A server that will not take the coding it was sent, answering as RFC 9110
				// describes: `415` naming what it would have taken instead.
				req.resume();
				req.on("end", () => {
					const payload = JSON.stringify({ refused: req.headers["content-encoding"] });
					res.writeHead(415, {
						"content-type": "application/json",
						"content-length": String(Buffer.byteLength(payload)),
						"accept-encoding": "gzip",
						"x-request-count": String(requests),
					});
					res.end(payload);
				});
				return;
			}

			case "redirect-to-sink": {
				if (parts[1]) {
					// Drained before answering so the redirect does not land mid-body and
					// cost the connection, which would be a reset rather than a redirect.
					req.resume();
					req.on("end", () => {
						res.writeHead(Number(parts[1]), { location: "/sink" });
						res.end();
					});
					return;
				}
				break;
			}

			case "echo": {
				const payload = JSON.stringify({
					method: req.method,
					headers: req.headers,
					requestCount: requests,
				});
				res.writeHead(200, {
					"content-type": "application/json",
					"content-length": String(Buffer.byteLength(payload)),
					"x-request-count": String(requests),
				});
				res.end(req.method === "HEAD" ? undefined : payload);
				return;
			}
		}

		res.writeHead(404, { "content-type": "text/plain" });
		res.end("not found");
	});

	server.on("connection", (socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));
	});

	return {
		payload: PAYLOAD,
		canEncode,
		requests: () => seen.slice(),
		count: () => requests,

		listen() {
			return new Promise((resolve, reject) => {
				server.once("error", reject);
				server.listen(0, "127.0.0.1", () => resolve(server.address().port));
			});
		},

		url(path) {
			const addr = server.address();
			return `http://${addr.address}:${addr.port}${path}`;
		},

		close() {
			return new Promise((resolve) => {
				// Agents pool their connections, so a bare close() never settles.
				for (const socket of sockets) socket.destroy();
				sockets.clear();
				server.close(resolve);
				setTimeout(resolve, 500).unref();
			});
		},
	};
}

module.exports = { createEncodingOrigin, PAYLOAD };
