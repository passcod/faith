/**
 * Response framing: chunked transfer encoding versus Content-Length.
 *
 * Requires CHUNKED, which HTTP/2 cannot provide — HTTP/2 frames bodies and has
 * no chunked encoding — so this dimension skips the h2 row. That skip is the
 * point: it demonstrates the capability model excluding a cell that genuinely
 * cannot run, rather than every cell running everywhere.
 *
 * This dimension has no separate negative case because its two routes are each
 * other's control: if Content-Length detection were stuck absent the sized
 * assertion fails, and if stuck present the chunked assertion fails. Either
 * direction of breakage is caught, so a third contrived route would add nothing.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "framing",
	requires: [C.CHUNKED, C.CONTENT_LENGTH],
	// Declared so the runner catches this dimension quietly losing its coverage:
	// tape counts a test that asserts nothing as a pass. No negative case here, so
	// no negativeAssertions.
	assertions: 4,

	async run(t, { url, agent }) {
		const chunked = await fetchWith(agent, `${url}/framing/chunked`);
		const chunkedBody = await chunked.text();
		t.equal(chunkedBody, PAYLOAD, "reassembles a chunked body");
		t.notOk(
			chunked.headers.get("content-length"),
			"a chunked response carries no Content-Length",
		);

		const sized = await fetchWith(agent, `${url}/framing/length`);
		const sizedBody = await sized.text();
		t.equal(sizedBody, PAYLOAD, "reads a Content-Length body");
		t.equal(
			sized.headers.get("content-length"),
			String(Buffer.byteLength(PAYLOAD)),
			"and reports the declared length",
		);
	},
};

function fetchWith(agent, target) {
	return fetch(target, { agent, timeout: 10000 });
}
