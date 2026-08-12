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
 * likely and repeat it. A machine that never once loses the race passes this without
 * having tested anything, which is the price of the scenario being a race at all; the
 * origin-side count at the end is what keeps it from passing because the origin
 * quietly did nothing.
 *
 * The two methods are held to different standards, which is the point of running
 * both. A GET is replayed, so it must never reach the caller as a failure. A POST is
 * not, because nothing in the error says whether the origin processed the request
 * before the connection went, so the caller sees it -- that is the deliberate
 * trade, and asserting POSTs always succeed would assert the opposite of the
 * intended behaviour. What a POST must never do is come back *wrong*, so its
 * volley is checked for bad answers rather than for failures.
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
 * Splits the two ways a request can go wrong, because the methods are judged on
 * different ones: `lost` is the connection dying and taking the request with it,
 * `wrong` is an answer that arrived and was not the right one. A POST is allowed to
 * be lost and never allowed to be wrong.
 *
 * Returns them rather than asserting per request: sixty-four assertions saying "fine"
 * bury the run, and the thing worth reading is which of them was not.
 */
async function fireVolley(url, agent, method) {
	const attempts = Array.from({ length: CONCURRENCY }, async (_, i) => {
		try {
			const res = await fetch(`${url}/idle/drop`, {
				agent,
				method,
				// Cloneable, so nothing about *this* body stops a client from retrying:
				// a POST left unretried here is a decision, not a body it could not
				// replay.
				body: method === "POST" ? PAYLOAD : undefined,
				timeout: 10_000,
			});
			const body = await res.text();
			if (res.status !== 200) return { wrong: `${method} ${i}: status ${res.status}` };
			if (body !== PAYLOAD) return { wrong: `${method} ${i}: body ${JSON.stringify(body)}` };
			return {};
		} catch (err) {
			return { lost: `${method} ${i}: ${err.code || ""} ${err.message}`.trim() };
		}
	});
	const results = await Promise.all(attempts);
	return {
		lost: results.map((r) => r.lost).filter(Boolean),
		wrong: results.map((r) => r.wrong).filter(Boolean),
	};
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
			// GETs, which are replayable, so none of these may reach the caller as a
			// failure however the race lands. Every one is answered in full and then
			// abandoned, so the pool ends the volley holding CONCURRENCY connections
			// the origin has already let go of.
			const fill = await fireVolley(url, own, "GET");
			const survived = [...fill.lost, ...fill.wrong];
			t.equal(
				survived.length,
				0,
				`round ${round}: every GET survived the connection under it going away` +
					(survived.length ? ` -- ${survived.join("; ")}` : ""),
			);

			// POSTs, reclaiming those connections with no delay whatsoever: the line
			// above is the closest this can get to reusing them before the client has
			// noticed they are gone. Losing one is the accepted cost of not replaying
			// a request the origin may have processed; answering one wrongly is not.
			const reclaim = await fireVolley(url, own, "POST");
			t.equal(
				reclaim.wrong.length,
				0,
				`round ${round}: and no POST came back with the wrong answer ` +
					`(${reclaim.lost.length} of ${CONCURRENCY} went down with the connection)` +
					(reclaim.wrong.length ? ` -- ${reclaim.wrong.join("; ")}` : ""),
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
