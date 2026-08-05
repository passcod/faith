/**
 * Oversized request headers: the refusal has to be actionable, and survivable.
 *
 * How a server refuses is not portable, and asserting a status was wrong: Apache
 * answers 431 and closes politely, while nginx and Node reset the connection
 * mid-request, which reaches the client as a transport error rather than a response.
 * Both are legitimate -- HTTP does not promise that a refusal is readable -- so this
 * asserts what a client owes its caller either way.
 *
 * Namely: the request settles rather than hanging, the outcome says what happened
 * (a 4xx, or an error that names itself, never an opaque throw or a bare success),
 * and the agent still works afterwards. That last one is the property with teeth: a
 * connection the server reset is in the pool, and a client that hands it to the next
 * request turns one rejected header into a broken agent.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "header-limits",
	requires: [C.HEADER_LIMITS],
	assertions: 5,

	async run(t, { url, agent, server }) {
		const limit = server.headerLimit ?? 1024;

		// Comfortably under, leaving room for the header name and the headers faith
		// sends of its own accord. This has to be accepted, or "over the limit is
		// refused" says nothing about size.
		const small = await fetch(`${url}/hello`, {
			agent,
			timeout: 10_000,
			headers: { "x-conformance-probe": "z".repeat(Math.floor(limit / 4)) },
		});
		t.equal(small.status, 200, "a header under the limit is accepted");
		t.equal(await small.text(), PAYLOAD, "and the response is the ordinary one");

		// Comfortably over. Reaching the next line at all is the no-hang result: the
		// per-request timeout here, and the cell timeout above it, are what fail
		// otherwise.
		const outcome = await refuse(`${url}/hello`, agent, limit);
		t.ok(
			outcome.kind === "error" || (outcome.status >= 400 && outcome.status < 500),
			`refused rather than accepted, and it settled: ${describe(outcome)}`,
		);
		t.ok(
			outcome.kind === "status" || Boolean(outcome.code),
			`and the refusal names itself, so a caller can tell it from a broken network: ${describe(outcome)}`,
		);

		// The pool must not be poisoned by whatever the server did to that connection.
		const after = await fetch(`${url}/hello`, { agent, timeout: 10_000 });
		const body = await after.text();
		t.ok(
			after.status === 200 && body === PAYLOAD,
			"and the agent still works, so a rejected header costs one request rather than all of them",
		);
	},
};

/** The refusal, as data: either a status or a named error, never a throw. */
async function refuse(target, agent, limit) {
	try {
		const res = await fetch(target, {
			agent,
			timeout: 10_000,
			headers: { "x-conformance-probe": "z".repeat(limit * 4) },
		});
		// Drained here rather than by the caller: a server that resets after the status
		// line fails on the body, and that is still a refusal, not a hang.
		try {
			await res.text();
		} catch (err) {
			return { kind: "error", code: err.code || err.name, status: res.status };
		}
		return { kind: "status", status: res.status };
	} catch (err) {
		return { kind: "error", code: err.code || err.name };
	}
}

function describe(outcome) {
	return outcome.kind === "status" ? `status ${outcome.status}` : `error ${outcome.code}`;
}
