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
 * Requests after the first: the upgrade lands on a subsequent request rather
 * than the one that learned about it.
 */
const WARMUPS = 3;

/**
 * The advertisement is verified by a background probe before any request is
 * routed to HTTP/3, so how many requests the upgrade takes depends on how fast
 * the probe's QUIC handshake completes, not on a fixed request count. Poll
 * with a pause rather than hammering, up to a deadline a slow runner still
 * fits inside.
 */
const UPGRADE_DEADLINE = 10_000;
const UPGRADE_POLL_INTERVAL = 100;

module.exports = {
	name: "HTTP/3 upgrade",
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
		const deadline = Date.now() + UPGRADE_DEADLINE;
		do {
			warm = await fetch(`${url}/hello`, { agent, timeout: 15_000 });
			body = await warm.text();
			if (warm.version === "HTTP/3.0") break;
			await new Promise((r) => setTimeout(r, UPGRADE_POLL_INTERVAL));
		} while (Date.now() < deadline);
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
