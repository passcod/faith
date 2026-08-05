/**
 * Server-initiated connection close: does the client notice and reconnect?
 *
 * The row is configured to close a connection after a known number of requests, and
 * this asks for more than that. A pooled client that reuses a connection the server
 * has finished with fails here, which is the whole point -- but only if the server
 * really did close one, so the `Connection: close` observation is what stops this
 * from being a test that every client passes by accident.
 *
 * HTTP/1 only, by way of KEEPALIVE_LIMIT: HTTP/2 ignores MaxKeepAliveRequests
 * entirely, and six requests over one h2 connection to a server set to close after
 * two all succeed on that same connection.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

/**
 * A fixed count, not derived from the row's limit.
 *
 * The runner compares the declared assertion count against what actually ran, and a
 * count computed from the same number the loop uses would agree with itself no
 * matter how many times the loop went round. So the requests are a constant, and the
 * row's limit is asserted to sit below it instead.
 */
const REQUESTS = 6;

module.exports = {
	name: "keepalive",
	requires: [C.KEEPALIVE_LIMIT],
	// Two per request, plus the limit check and the close observation.
	assertions: REQUESTS * 2 + 2,

	async run(t, { url, agent, server }) {
		const total = REQUESTS;
		t.ok(
			(server.keepaliveLimit ?? Infinity) < total,
			`the row closes after ${server.keepaliveLimit}, which is inside the ${total} ` +
				"requests below -- otherwise none of them crosses a close",
		);

		let sawClose = false;

		for (let i = 1; i <= total; i++) {
			const res = await fetch(`${url}/hello`, { agent, timeout: 10_000 });
			if ((res.headers.get("connection") || "").toLowerCase().includes("close")) {
				sawClose = true;
			}
			const body = await res.text();
			t.equal(res.status, 200, `request ${i} of ${total} succeeded`);
			t.equal(body, PAYLOAD, `request ${i} of ${total} carried the payload`);
		}

		t.ok(
			sawClose,
			"and the server really did close a connection along the way, so the requests " +
				"above crossed one",
		);
	},
};
