/**
 * The origin contract: the paths every row serves, and what each one is for.
 *
 * Dimensions fetch these paths; servers implement them. Neither imports the
 * other, because a dimension that reached into a particular server's module for
 * its payload would only ever be able to run against that server.
 *
 * Each route names the capabilities that oblige a server to serve it. That makes
 * a mismatched declaration -- claiming GZIP with no `/encoding/gzip` route --
 * fail in the server's own selftest, where the diagnosis is obvious, rather than
 * inside a dimension, where a 404 arrives as some unrelated assertion failing.
 */

const { CAPABILITIES: C } = require("./capabilities.js");

const PAYLOAD = "conformance-payload";
const TRAILER_NAME = "x-conformance-checksum";
const TRAILER_VALUE = "abc123";

/** The ETag `/conditional/etag` must carry, so a client can send it back. */
const ETAG = '"conformance-etag"';

/**
 * The body of the encoding routes, kept separate from PAYLOAD and deliberately
 * large.
 *
 * Every configured server refuses to compress a body below some threshold, because
 * gzip framing costs more than it saves: mod_deflate stops at 20 bytes ("Not
 * compressing very small response of 19 bytes", which is exactly PAYLOAD), and
 * Caddy's `encode` defaults to 512. Under those thresholds the server sends plain
 * bytes and the encoding dimension's assertions -- the body round-trips, no
 * Content-Encoding survives -- all pass without a single byte having been
 * compressed.
 *
 * So this clears every threshold, and compresses hard enough that the encoded
 * length is unmistakably smaller than the decoded one, which is what lets the
 * dimension prove compression happened rather than assume it.
 */
const COMPRESSIBLE = `${"conformance-compressible-body\n".repeat(40)}`;

/**
 * `requires` is the full set a server needs before this route is expected of it.
 *
 * The deliberately-wrong routes carry their dimension's capability as well as
 * SCRIPTABLE, so a server that cannot do the correct behaviour is never asked for
 * the broken one: Caddy can be configured to mislabel a content coding, and can
 * never emit a trailer at all.
 */
const ROUTES = [
	{ path: "/hello", requires: [], what: "baseline; every row serves this" },
	{ path: "/trailers", requires: [C.TRAILERS], what: "body then a trailer" },
	{
		path: "/trailers/omitted",
		requires: [C.TRAILERS, C.SCRIPTABLE],
		what: "declares a trailer in the Trailer header and sends none",
	},
	{ path: "/framing/chunked", requires: [C.CHUNKED], what: "no Content-Length" },
	{ path: "/framing/length", requires: [C.CONTENT_LENGTH], what: "sized body" },
	{ path: "/encoding/gzip", requires: [C.GZIP], what: "gzip-encoded body" },
	{
		path: "/encoding/mislabelled",
		requires: [C.GZIP, C.SCRIPTABLE],
		what: "claims gzip, sends plain text",
	},
	{
		path: "/conditional/etag",
		requires: [C.CONDITIONAL],
		what: "carries an ETag and answers 304 to a matching If-None-Match",
	},
	{
		path: "/goaway",
		requires: [C.GOAWAY],
		what: "answers normally, then sends an HTTP/2 GOAWAY",
	},
	{
		path: "/goaway/state",
		requires: [C.GOAWAY],
		what: "reports how many GOAWAYs and sessions the origin has seen",
	},
];

/** The routes a server declaring `capabilities` is obliged to serve. */
function routesFor(capabilities) {
	return ROUTES.filter((route) => route.requires.every((c) => capabilities.has(c)));
}

module.exports = {
	PAYLOAD,
	COMPRESSIBLE,
	TRAILER_NAME,
	TRAILER_VALUE,
	ETAG,
	ROUTES,
	routesFor,
};
