/**
 * Routes for the Node origin, shared by its HTTP/1 and HTTP/2 listeners.
 *
 * Every route is either a correct behaviour a dimension asserts, or a
 * deliberately wrong one so that dimension's negative case can prove the
 * assertion is capable of failing. A conformance test that cannot fail is
 * decoration.
 */

const zlib = require("node:zlib");

const { PAYLOAD, COMPRESSIBLE, TRAILER_NAME, TRAILER_VALUE, ETAG } = require("../contract.js");

/**
 * What the origin has done, readable by a client.
 *
 * The h2-GOAWAY dimension cannot otherwise tell a working GOAWAY from an absent one:
 * nothing faith exposes says which connection a request used, so "the response
 * arrived and the next request worked" is equally true of a server that sent no
 * GOAWAY at all. Counting sessions here turns that into something observable -- a
 * client that honoured the frame opens a new session, one that reused the retired
 * connection does not.
 *
 * Process-wide and never reset, because each cell starts its own server while this
 * module stays loaded. Readers compare deltas, not absolute values.
 */
const state = { goaways: 0, sessions: 0, answered: 0, dropped: 0 };

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
			// The same large body the configured rows serve here, so every row answers
			// the same assertions.
			res.setHeader("content-type", "text/plain");
			res.setHeader("vary", "accept-encoding");

			// Negotiated rather than always-gzip, because the dimension's control asks
			// for identity and compares. A route that gzipped regardless would answer
			// both requests identically, and the comparison would prove nothing about
			// either.
			if (!/\bgzip\b/.test(req.headers["accept-encoding"] || "")) {
				res.setHeader("content-length", String(Buffer.byteLength(COMPRESSIBLE)));
				res.end(COMPRESSIBLE);
				return;
			}

			const body = zlib.gzipSync(Buffer.from(COMPRESSIBLE));
			res.setHeader("content-encoding", "gzip");
			res.setHeader("content-length", String(body.length));
			res.end(body);
			return;
		}

		// claims gzip but sends plain text: the negative case for the encoding
		// dimension, since a client that really decompresses must fail here
		case "/encoding/mislabelled":
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-encoding", "gzip");
			res.end(COMPRESSIBLE);
			return;

		// --- HTTP/2 GOAWAY ---
		// Answers normally and then tells the client this connection is finished. No
		// configured server in the matrix can be made to do this on demand: Apache and
		// HAProxy send GOAWAY on their own schedule, which is not something a test can
		// assert against.
		case "/goaway":
			res.setHeader("content-type", "text/plain");
			res.end(PAYLOAD);
			// After the response, so the request that triggers it is not the one that
			// gets torn down: the dimension asserts that this response arrives intact
			// *and* that the next request survives the connection going away.
			if (res.stream && res.stream.session) {
				const { session } = res.stream;
				// On the next tick, so the response has left the building. goaway() on a
				// session already closing throws, hence the guard.
				setImmediate(() => {
					if (!session.destroyed && !session.closed) {
						session.goaway();
						state.goaways++;
					}
				});
			}
			return;

		// What the origin has done, so a dimension can assert on it rather than on
		// effects it has no way to see.
		case "/goaway/state":
			res.setHeader("content-type", "application/json");
			res.end(JSON.stringify(state));
			return;

		// --- aggressive idle close ---
		// Answers in full, then closes the connection without ever having said it
		// would. This is the origin behaviour the pooling client cannot see coming:
		// the response is complete and unremarkable, so the connection goes back
		// into the pool, and the FIN lands some unknowable moment later.
		//
		// Both directions, in that order, and neither half is optional. `end()`
		// first because it is graceful: it flushes the response before the FIN,
		// where destroying outright can reset the connection with bytes still in
		// the TLS buffer and turn this into a truncation test. Then `destroy()`
		// once that flush has completed, because `end()` alone is a *half*-close --
		// Node keeps the read side open, so it goes on parsing and handling
		// requests written into the socket afterwards that it can no longer answer.
		// A real origin closing an idle connection shuts both directions, and those
		// later requests never reach it at all. Left half-closed, this route
		// measures a scenario no origin in the wild produces, and the requests it
		// swallows are ones the origin really did process.
		case "/idle/drop": {
			// POSTs come through here, and an unread request body leaves the stream
			// paused and the socket unable to finish.
			req.resume();
			const socket = req.socket;
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-length", String(Buffer.byteLength(PAYLOAD)));
			// Counted where the request is parsed, so it counts requests the origin
			// really received: one written into a socket the origin had already
			// abandoned never gets here, which is the whole point.
			state.answered++;
			res.end(PAYLOAD, () => {
				// `socket` is captured above rather than read as `res.socket` here:
				// Node detaches the socket from the response as it finishes, so
				// `res.socket` in this callback is null and the close silently never
				// happens. That failure mode is indistinguishable from a client that
				// survived every close, which is why the dimension asserts this
				// counter rather than assuming the route did its job.
				state.dropped++;
				socket.end();
				socket.once("finish", () => socket.destroy());
			});
			return;
		}

		case "/idle/state":
			res.setHeader("content-type", "application/json");
			res.end(JSON.stringify(state));
			return;

		// --- conditional requests ---
		// A fixed ETag rather than one derived from the body: the dimension asserts
		// the validator round-trips, and a stable value means a failure points at
		// the round-trip rather than at how the ETag was computed.
		case "/conditional/etag": {
			res.setHeader("etag", ETAG);
			if (req.headers["if-none-match"] === ETAG) {
				// No Content-Length and no body: a 304 that carried either would be a
				// different bug, and the dimension asserts the body is empty.
				res.statusCode = 304;
				res.end();
				return;
			}
			res.setHeader("content-type", "text/plain");
			res.setHeader("content-length", String(Buffer.byteLength(PAYLOAD)));
			res.end(PAYLOAD);
			return;
		}

		default:
			res.statusCode = 404;
			res.end("no such route");
			return;
	}
}

module.exports = { handle, state };
