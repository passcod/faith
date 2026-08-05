/**
 * Response trailers.
 *
 * No real server in the matrix can emit trailers on demand, so this dimension
 * only ever runs against the controllable origin — which is precisely why it is
 * worth having: nothing else in faith's test suite covers trailers at all.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD, TRAILER_NAME, TRAILER_VALUE } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "trailers",
	requires: [C.TRAILERS],
	// Declared so the runner catches this dimension quietly losing its coverage:
	// tape counts a test that asserts nothing as a pass.
	assertions: 3,
	negativeAssertions: 1,

	async run(t, { url, agent }) {
		const res = await fetchWith(agent, `${url}/trailers`);

		// Consume the body FIRST. faith resolves `trailers` only once the body
		// stream completes: the native side polls a NotYet state and the value is
		// set either by a trailers frame or by a sentinel appended after the last
		// body chunk. Awaiting trailers before draining the body therefore spins
		// forever rather than erroring.
		const body = await res.text();
		t.equal(body, PAYLOAD, "body arrives ahead of the trailers");

		const trailers = await res.trailers;
		t.ok(trailers, "trailers are exposed once the body is consumed");
		t.equal(
			trailers && trailers.get(TRAILER_NAME),
			TRAILER_VALUE,
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
		t.equal(
			trailers,
			null,
			"no trailers arrived at all -- not a fabricated entry with an empty value, \
which a falsy-value check would have accepted",
		);
	},
};

function fetchWith(agent, target) {
	return fetch(target, { agent, timeout: 10000 });
}
