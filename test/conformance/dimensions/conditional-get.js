/**
 * Conditional requests: does a validator round-trip and actually get validated?
 *
 * The bogus-validator request is what makes this discriminating. A server that
 * answered 304 unconditionally, or a client that mixed up which validator it sent,
 * would satisfy the matching case perfectly well -- so the dimension asks for both
 * answers and would notice if only one of them were ever produced.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

const PATH = "/conditional/etag";

module.exports = {
	name: "conditional GET",
	requires: [C.CONDITIONAL],
	// Declared so the runner catches this dimension quietly losing its coverage:
	// tape counts a test that asserts nothing as a pass. No separate negative case:
	// the two validators are each other's control, so breakage in either direction
	// -- always 304, never 304 -- fails one of these assertions.
	assertions: 6,

	async run(t, { url, agent }) {
		const first = await fetch(`${url}${PATH}`, { agent, timeout: 10_000 });
		t.equal(await first.text(), PAYLOAD, "serves the resource");

		const etag = first.headers.get("etag");
		t.ok(etag, "with an ETag to validate against");

		const matched = await fetch(`${url}${PATH}`, {
			agent,
			timeout: 10_000,
			headers: { "if-none-match": etag },
		});
		// Read the body before asserting on it, not after: a 304 must carry none, and
		// leaving it unread would hold the pooled connection for the next request.
		const matchedBody = await matched.text();
		t.equal(matched.status, 304, "answers 304 to a matching If-None-Match");
		t.equal(matchedBody, "", "and sends no body with it");

		const mismatched = await fetch(`${url}${PATH}`, {
			agent,
			timeout: 10_000,
			headers: { "if-none-match": '"not-the-etag"' },
		});
		const mismatchedBody = await mismatched.text();
		t.equal(mismatched.status, 200, "but 200 to a validator that does not match");
		t.equal(mismatchedBody, PAYLOAD, "with the resource again, so the 304 was validation");
	},
};
