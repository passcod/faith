/**
 * Content coding: does the client actually decompress what the server labelled?
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../servers/controllable-routes.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "encoding",
	requires: [C.GZIP],

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
		// Labelled gzip, sent as plain text. A client that really decompresses
		// must fail; one that passes the bytes through would happily return them.
		let failed = false;
		try {
			const res = await fetchWith(agent, `${url}/encoding/mislabelled`);
			await res.text();
		} catch {
			failed = true;
		}
		t.ok(failed, "a body mislabelled as gzip is rejected rather than passed through");
	},
};

function fetchWith(agent, target) {
	return fetch(target, { agent, timeout: 10000 });
}
