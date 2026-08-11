/**
 * Origins that drop pooled connections without warning: does the caller ever see it?
 *
 * The `connection reuse` dimension covers the polite case -- the server says
 * `Connection: close` and the client is told the connection is finished. This is the
 * impolite one. The response is complete and says nothing, so the connection goes
 * back into the pool looking healthy, and the FIN arrives some moment afterwards. A
 * client that hands out a connection the origin has already abandoned surfaces that
 * to the caller as a connection error on a request the origin never saw.
 *
 * The window between the connection returning to the pool and the client noticing
 * the close is what decides this, and nothing here can open it on demand -- both ends
 * are racing on loopback. So the shape is: fill the pool, let the origin abandon
 * every connection in it at once, then immediately ask for all of them back. Losing
 * the race is the interesting outcome and this cannot force it, but it can make it
 * likely and repeat it, and the assertion is the same either way: no caller-visible
 * failure, whichever way the race lands. A machine that never once loses the race
 * passes this without having tested anything, which is the price of the scenario
 * being a race at all; the origin-side count below is what keeps it from passing
 * because the origin quietly did nothing.
 *
 * POSTs, not just GETs. A retry after a connection dies is only safe for a
 * non-idempotent request if nothing was ever written to the wire, so a client that
 * gets the condition wrong -- retrying on "no response arrived" rather than on "the
 * request was never sent" -- would either double-send here or refuse to retry and
 * fail the request. Both are visible from this side.
 *
 * HTTP/1 only. The premise is a pool holding one connection per in-flight request;
 * a multiplexed h2 session that goes away is the `h2 GOAWAY` dimension.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

/**
 * How many connections to have the origin abandon at once.
 *
 * Concurrency is the amplifier: one request racing one FIN mostly loses, and the
 * client reconnects with nothing interesting having happened. Eight at once means
 * eight independent draws per round against pool slots that all went bad together.
 */
const CONCURRENCY = 8;

/** Rounds of fill-and-abandon. More draws at the race, for the same reason. */
const ROUNDS = 4;

/**
 * Fire `CONCURRENCY` requests at the dropping route and describe what went wrong.
 *
 * Returns the failures rather than asserting per request: sixty-four assertions
 * saying "fine" bury the run, and the thing worth reading is which of them was not.
 * A throw is a failure like any other here -- a dead pooled connection surfaces as a
 * rejected fetch, which is precisely the symptom under test.
 */
async function fireVolley(url, agent, method) {
	const attempts = Array.from({ length: CONCURRENCY }, async (_, i) => {
		try {
			const res = await fetch(`${url}/idle/drop`, {
				agent,
				method,
				// Cloneable, so nothing about *this* body stops a client from retrying.
				body: method === "POST" ? PAYLOAD : undefined,
				timeout: 10_000,
			});
			const body = await res.text();
			if (res.status !== 200) return `${method} ${i}: status ${res.status}`;
			if (body !== PAYLOAD) return `${method} ${i}: body ${JSON.stringify(body)}`;
			return null;
		} catch (err) {
			return `${method} ${i}: ${err.code || ""} ${err.message}`.trim();
		}
	});
	return (await Promise.all(attempts)).filter(Boolean);
}

/** What the origin says it has done. Deltas, since the counters are process-wide. */
async function readState(url, agent) {
	const res = await fetch(`${url}/idle/state`, { agent, timeout: 10_000 });
	return JSON.parse(await res.text());
}

module.exports = {
	name: "aggressive idle close",
	requires: [C.IDLE_CLOSE, C.H1],
	// Two volleys per round, plus the origin-really-dropped-them check.
	assertions: ROUNDS * 2 + 1,

	async run(t, { url, makeAgent }) {
		// Its own agent, so the pool starts empty. The runner's shared agent has
		// already been through the version probe, and a warm connection to a route
		// that does not drop anything is one this cannot account for.
		const own = makeAgent();
		const before = await readState(url, own);

		for (let round = 1; round <= ROUNDS; round++) {
			// Fill: every one of these is answered in full and then abandoned, so the
			// pool ends the volley holding CONCURRENCY connections the origin has
			// already let go of.
			const fill = await fireVolley(url, own, "GET");
			t.equal(
				fill.length,
				0,
				`round ${round}: the volley that fills the pool succeeded` +
					(fill.length ? ` -- ${fill.join("; ")}` : ""),
			);

			// Reclaim, with no delay whatsoever: the line above is the closest this
			// can get to reusing those connections before the client has noticed they
			// are gone.
			const reclaim = await fireVolley(url, own, "POST");
			t.equal(
				reclaim.length,
				0,
				`round ${round}: and the POSTs that reuse the pool did too` +
					(reclaim.length ? ` -- ${reclaim.join("; ")}` : ""),
			);
		}

		// The load-bearing observation, and the reason the route reports at all: an
		// origin that answered normally and quietly kept its connections open would
		// satisfy every assertion above without the scenario under test having
		// happened once. Counted against what the origin answered rather than what
		// this asked for, so it says the same thing whether or not the client failed.
		const after = await readState(url, own);
		const answered = after.answered - before.answered;
		const dropped = after.dropped - before.dropped;
		t.equal(
			dropped,
			answered,
			`and the origin abandoned the connection under every one of the ` +
				`${answered} requests it answered`,
		);
	},
};
