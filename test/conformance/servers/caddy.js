/**
 * The Caddy row.
 *
 * Caddy is in the matrix for two things nothing else in it has: a complete
 * HTTP/3 + Alt-Svc upgrade path, and Go's `net/http2` rather than nghttp2. It is
 * also what the downstream consumer whose bug report started #23 actually runs,
 * so its behaviour is the one that matters most in practice.
 *
 * No CHUNKED: a static file server knows the length and sends Content-Length. The
 * only way Caddy produces a chunked body is as a side effect of compressing on the
 * fly, which would make the chunked-bodies assertions secretly about gzip. Chunked framing
 * is tested on the proxy row instead, where it is the interesting case anyway.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { ensureCert, findFreePort } = require("../../fixtures/net.js");
const { startCaddy, caddyAvailable } = require("../../fixtures/caddy.js");
const { buildStaticTree } = require("./static-tree.js");

/**
 * Compression is scoped to the one route that asks for it. Enabling `encode`
 * site-wide would strip Content-Length from every response, so the row would stop
 * being able to demonstrate Content-Length framing at all -- and the encoding
 * dimension would pass for a reason it never asserted.
 */
function directivesFor(dir) {
	return [
		// Absolute and quoted: a relative root would resolve against Caddy's working
		// directory, which is this test run's, not the tree's.
		`root * "${dir}"`,
		"@gzip path /encoding/gzip",
		"encode @gzip gzip",
		// Labels a plain file as gzip: the gzip dimension's negative case.
		"@mislabelled path /encoding/mislabelled",
		"header @mislabelled Content-Encoding gzip",
		"file_server",
	];
}

const caddy = {
	name: "caddy",
	// Both http/1.1 and h2 are offered, so this asserts the client's preference
	// rather than the server's only option -- see the protocol-negotiation dimension.
	expectVersion: "HTTP/2.0",
	capabilities: new Set([
		C.H1,
		C.H2,
		C.H3,
		C.ALTSVC,
		C.ALPN_MULTI,
		C.TLS,
		C.GZIP,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		C.SCRIPTABLE,
	]),
	available: caddyAvailable,

	async start() {
		const { ca } = ensureCert();
		const port = await findFreePort();
		const tree = buildStaticTree();
		const running = await startCaddy({
			port,
			dir: tree.dir,
			directives: directivesFor(tree.dir),
		});
		return {
			url: `https://localhost:${port}`,
			ca,
			log: running.log,
			close: async () => {
				running.close();
				tree.cleanup();
			},
		};
	},
};

module.exports = { caddy };
