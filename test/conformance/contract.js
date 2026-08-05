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
];

/** The routes a server declaring `capabilities` is obliged to serve. */
function routesFor(capabilities) {
	return ROUTES.filter((route) => route.requires.every((c) => capabilities.has(c)));
}

module.exports = { PAYLOAD, TRAILER_NAME, TRAILER_VALUE, ETAG, ROUTES, routesFor };
