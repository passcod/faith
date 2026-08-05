/**
 * Content coding: does the client actually decompress what the server labelled?
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "encoding",
	requires: [C.GZIP],
	// Declared so the runner catches this dimension quietly losing its coverage:
	// tape counts a test that asserts nothing as a pass.
	assertions: 2,
	negativeAssertions: 2,

	async run(t, { url, agent }) {
		const res = await fetchWith(agent, `${url}/encoding/gzip`);
		const body = await res.text();
		t.equal(body, PAYLOAD, "transparently decompresses gzip");
		t.notOk(
			res.headers.get("content-encoding"),
			"and strips Content-Encoding once decoded, so the body matches the header",
		);
	},

	async negative(t, { url, agent }) {
		// Labelled gzip, sent as plain text. A client that really decompresses must
		// fail; one that passes the bytes through would happily return them.
		//
		// Split the fetch from the body read, because that is what makes this
		// discriminating. Decoding happens while streaming the body, so a
		// mislabelled body yields perfectly good headers and fails only on read --
		// whereas a connect, TLS or timeout failure would reject the fetch itself.
		// Wrapping both in one try/catch would let any of those satisfy the
		// assertion. faith's body-stream errors carry no `code` property, so this
		// separation is the discriminator available to us, not error matching.
		const res = await fetchWith(agent, `${url}/encoding/mislabelled`);
		t.equal(
			res.status,
			200,
			"the response itself arrives -- so a later failure is about the body, not the connection",
		);

		let failed = false;
		try {
			await res.text();
		} catch {
			failed = true;
		}
		t.ok(
			failed,
			"reading a body mislabelled as gzip fails rather than yielding the raw bytes",
		);
	},
};

function fetchWith(agent, target) {
	return fetch(target, { agent, timeout: 10000 });
}
