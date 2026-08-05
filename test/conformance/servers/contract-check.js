/**
 * The assertion every server's selftest shares: it serves what it claims.
 *
 * A capability declaration is a promise about routes. Break the promise and the
 * dimension that needs the route fetches a 404 and fails on whatever it asserts
 * about the body -- a wrong-looking payload, a missing header -- which reads as a
 * faith bug rather than as a misconfigured row. Checking the promise directly, in
 * the server's own selftest, puts the failure where the mistake is.
 */

const { routesFor } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

/** Asserts once per obliged route. Returns how many, so a caller can say so. */
async function assertServesContract(t, { url, agent, capabilities }) {
	const routes = routesFor(capabilities);
	for (const route of routes) {
		const res = await fetch(`${url}${route.path}`, { agent, timeout: 10_000 });

		// Drain before the next request. An unread body leaves the pooled connection
		// busy, so the following fetch waits on a slot that never frees -- and the
		// whole selftest hangs instead of failing. The read is allowed to throw:
		// `/encoding/mislabelled` is *meant* to be undecodable, and this check is
		// about the status line, not the body.
		try {
			await res.text();
		} catch {}

		t.notEqual(res.status, 404, `serves ${route.path}: ${route.what}`);
	}
	return routes.length;
}

module.exports = { assertServesContract };
