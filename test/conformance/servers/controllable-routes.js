/**
 * Routes for the controllable origin, shared by its HTTP/1 and HTTP/2 listeners.
 *
 * Every route is either a correct behaviour a dimension asserts, or a
 * deliberately wrong one so that dimension's negative case can prove the
 * assertion is capable of failing. A conformance test that cannot fail is
 * decoration.
 */

const zlib = require("node:zlib");

const PAYLOAD = "conformance-payload";
const TRAILER_NAME = "x-conformance-checksum";
const TRAILER_VALUE = "abc123";

function handle(req, res) {
	const url = new URL(req.url, "https://localhost");

	switch (url.pathname) {
		// --- baseline ---
		case "/hello":
			res.setHeader("content-type", "text/plain");
			res.end(PAYLOAD);
			return;

		// --- trailers ---
		case "/trailers":
			res.setHeader("content-type", "text/plain");
			res.setHeader("trailer", TRAILER_NAME);
			res.write(PAYLOAD);
			res.addTrailers({ [TRAILER_NAME]: TRAILER_VALUE });
			res.end();
			return;

		// declares a trailer in the Trailer header and then sends none: the
		// negative case for the trailers dimension
		case "/trailers/omitted":
			res.setHeader("content-type", "text/plain");
			res.setHeader("trailer", TRAILER_NAME);
			res.write(PAYLOAD);
			res.end();
			return;

		// --- framing ---
		// no content-length, so HTTP/1 must chunk this
		case "/framing/chunked":
			res.setHeader("content-type", "text/plain");
			res.write(PAYLOAD.slice(0, 5));
			res.write(PAYLOAD.slice(5));
			res.end();
			return;

		case "/framing/length":
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-length", String(Buffer.byteLength(PAYLOAD)));
			res.end(PAYLOAD);
			return;

		// --- encoding ---
		case "/encoding/gzip": {
			const body = zlib.gzipSync(Buffer.from(PAYLOAD));
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-encoding", "gzip");
			res.setHeader("vary", "accept-encoding");
			res.end(body);
			return;
		}

		// claims gzip but sends plain text: the negative case for the encoding
		// dimension, since a client that really decompresses must fail here
		case "/encoding/mislabelled":
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-encoding", "gzip");
			res.end(PAYLOAD);
			return;

		default:
			res.statusCode = 404;
			res.end("no such route");
			return;
	}
}

module.exports = { handle, PAYLOAD, TRAILER_NAME, TRAILER_VALUE };
