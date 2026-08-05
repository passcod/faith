/**
 * ALPN: which protocol does the client pick when the server offers a choice?
 *
 * Distinct from the runner's per-row version probe, which asserts that a row
 * negotiates the protocol its own module declares. That check follows the
 * declaration -- change `expectVersion` and it moves with it. This one does not:
 * offered both http/1.1 and h2, faith is expected to take h2, and a row that
 * quietly stopped doing so fails here regardless of what it claims.
 *
 * No negative case, because the HTTP/1-only Node row is the control: it
 * offers http/1.1 alone and its probe asserts HTTP/1.1, so a client stuck on either
 * version fails on one row or the other.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "protocol negotiation",
	requires: [C.ALPN_MULTI],
	// Declared so the runner catches this dimension quietly losing its coverage:
	// tape counts a test that asserts nothing as a pass.
	assertions: 2,

	async run(t, { url, agent }) {
		const res = await fetch(`${url}/hello`, { agent, timeout: 10_000 });
		t.equal(await res.text(), PAYLOAD, "the offered-both row serves the payload");
		t.equal(res.version, "HTTP/2.0", "and the client prefers h2 over http/1.1");
	},
};
