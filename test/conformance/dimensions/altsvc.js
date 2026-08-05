/**
 * The Alt-Svc upgrade path: TCP first, then HTTP/3 because the server said so.
 *
 * This is the path #23 broke -- a wedged agent that never came back to TCP after the
 * UDP path died -- and it is covered by regression tests against one server. Here it
 * becomes a property of the matrix, asked of any row that advertises.
 *
 * The negative case is what makes the positive one mean something: with upgrades
 * turned off, the same row over the same requests must stay on TCP. Without it, a
 * client that spoke HTTP/3 for its own reasons would pass.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

/**
 * Requests after the first, matching what the HTTP/3 regression tests do: the
 * upgrade lands on a subsequent request rather than the one that learned about it.
 */
const WARMUPS = 3;

module.exports = {
	name: "altsvc",
	requires: [C.ALTSVC],
	assertions: 4,
	negativeAssertions: 2,

	async run(t, { url, makeAgent }) {
		const agent = makeAgent({ http3: { upgradeEnabled: true } });

		// No hint seeded: the point is that the advertisement is what teaches the client,
		// so this first request has to go over TCP and come back carrying it.
		const first = await fetch(`${url}/hello`, { agent, timeout: 15_000 });
		t.equal(await first.text(), PAYLOAD, "the first request is served over TCP");
		t.ok(first.headers.get("alt-svc"), "and advertises HTTP/3 in Alt-Svc");
		t.notEqual(
			first.version,
			"HTTP/3.0",
			"the advertisement itself arrived over TCP, which is the only way it can",
		);

		let warm;
		let body;
		for (let i = 0; i < WARMUPS; i++) {
			warm = await fetch(`${url}/hello`, { agent, timeout: 15_000 });
			body = await warm.text();
		}
		t.ok(
			warm.version === "HTTP/3.0" && body === PAYLOAD,
			`a later request took the advertised route: ${warm.version}`,
		);
	},

	async negative(t, { url, makeAgent }) {
		const agent = makeAgent({ http3: { upgradeEnabled: false } });

		let last;
		let body;
		for (let i = 0; i <= WARMUPS; i++) {
			last = await fetch(`${url}/hello`, { agent, timeout: 15_000 });
			body = await last.text();
		}
		t.notEqual(
			last.version,
			"HTTP/3.0",
			"with upgrades off, the same row over the same requests stays on TCP",
		);
		t.equal(body, PAYLOAD, "and still serves the payload");
	},
};
