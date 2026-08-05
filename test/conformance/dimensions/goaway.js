/**
 * HTTP/2 GOAWAY: the server retires a connection the client is pooling.
 *
 * A GOAWAY is not an error — it means "finish what you have, start nothing new
 * here". So the response that triggers it must still arrive whole, and the request
 * after it must succeed on a replacement connection. A client that treats the frame
 * as a failure breaks the first; one that keeps handing work to a retired connection
 * breaks the second.
 *
 * Runs only on the node-h2 row: a server has to be told exactly when to send
 * one, and Apache and HAProxy send theirs on their own schedule.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

/** What the origin says it has done. Deltas, since the counters are process-wide. */
async function readState(url, agent) {
	const res = await fetch(`${url}/goaway/state`, { agent, timeout: 10_000 });
	return JSON.parse(await res.text());
}

module.exports = {
	name: "h2 GOAWAY",
	requires: [C.GOAWAY, C.H2],
	assertions: 7,

	async run(t, { url, agent }) {
		// Nothing faith exposes says which connection carried a request, so without
		// asking the origin this dimension would pass identically against a server that
		// sent no GOAWAY at all: the response arrives, the next request works, done.
		const before = await readState(url, agent);

		const triggered = await fetch(`${url}/goaway`, { agent, timeout: 10_000 });
		t.equal(triggered.status, 200, "the response that triggers the GOAWAY arrives");
		t.equal(await triggered.text(), PAYLOAD, "with its body intact");
		t.equal(triggered.version, "HTTP/2.0", "over h2, which is the only place GOAWAY exists");

		// Two afterwards, not one: the first proves a replacement connection can be
		// established, the second proves the replacement is itself reusable -- a client
		// that reconnected but left the pool holding the retired connection would pass
		// with only one.
		for (const attempt of [1, 2]) {
			const after = await fetch(`${url}/hello`, { agent, timeout: 10_000 });
			const body = await after.text();
			// Asserted together, because a request that fails after a GOAWAY tends to
			// fail at the status, and one that got the wrong connection tends to fail at
			// the body.
			t.ok(
				after.status === 200 && body === PAYLOAD,
				`request ${attempt} after the GOAWAY succeeded on a fresh connection`,
			);
		}

		const after = await readState(url, agent);
		t.equal(after.goaways - before.goaways, 1, "the origin did send exactly one GOAWAY");
		// The client's side of honouring it: a session it was told to stop using must be
		// replaced, not reused. Reusing it would leave this at zero while every assertion
		// above still passed.
		t.ok(
			after.sessions - before.sessions >= 1,
			`and the client opened a fresh session rather than reusing the retired one ` +
				`(+${after.sessions - before.sessions})`,
		);
	},
};
