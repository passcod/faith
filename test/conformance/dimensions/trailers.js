/**
 * Response trailers.
 *
 * No real server in the matrix can emit trailers on demand, so this dimension
 * only ever runs against the controllable origin — which is precisely why it is
 * worth having: nothing else in faith's test suite covers trailers at all.
 */

const { CAPABILITIES: C } = require("../capabilities.js");

module.exports = {
	name: "trailers",
	requires: [C.TRAILERS],

	async run(t, { url, agent }) {
		const res = await fetchWith(agent, `${url}/trailers`);

		// Consume the body FIRST. faith resolves `trailers` only once the body
		// stream completes: the native side polls a NotYet state and the value is
		// set either by a trailers frame or by a sentinel appended after the last
		// body chunk. Awaiting trailers before draining the body therefore spins
		// forever rather than erroring.
		const body = await res.text();
		t.equal(body, "conformance-payload", "body arrives ahead of the trailers");

		const trailers = await res.trailers;
		t.ok(trailers, "trailers are exposed once the body is consumed");
		t.equal(
			trailers && trailers.get("x-conformance-checksum"),
			"abc123",
			"and carry the value the server sent",
		);
	},

	async negative(t, { url, agent }) {
		// The server declares a trailer in the Trailer header and then sends none.
		// A client that genuinely reads trailers must report their absence rather
		// than inventing them from the declaration.
		const res = await fetchWith(agent, `${url}/trailers/omitted`);
		await res.text();
		const trailers = await res.trailers;
		const value = trailers && trailers.get("x-conformance-checksum");
		t.notOk(
			value,
			"a declared-but-unsent trailer is absent, not fabricated from the Trailer header",
		);
	},
};

function fetchWith(agent, target) {
	const { fetch } = require("../../../wrapper.js");
	return fetch(target, { agent, timeout: 10000 });
}
