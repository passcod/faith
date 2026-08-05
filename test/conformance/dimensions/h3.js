/**
 * HTTP/3 over QUIC, against a server that is not quinn.
 *
 * faith's client stack is quinn, so a row running quinn would be testing one
 * implementation against itself. Caddy brings quic-go and quiche brings
 * Cloudflare's, and this dimension holds both to the same assertions the TCP rows
 * answer.
 *
 * The agent gets a seeded Alt-Svc hint rather than being left to discover HTTP/3:
 * quiche-server has no TCP listener at all, so there is no response for it to learn
 * an advertisement from. Discovery is what the altsvc dimension covers.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "h3",
	requires: [C.H3],
	// No negative case: an HTTP/3 request that silently fell back to TCP fails the
	// version assertion, and one that failed outright never reaches it.
	assertions: 4,

	async run(t, { url, makeAgent }) {
		const port = Number(new URL(url).port);
		const agent = makeAgent({
			http3: { upgradeEnabled: true, hints: [{ host: "localhost", port }] },
		});

		const first = await fetch(`${url}/hello`, { agent, timeout: 15_000 });
		t.equal(await first.text(), PAYLOAD, "an HTTP/3 request returns the payload");
		t.equal(first.version, "HTTP/3.0", "over HTTP/3 from the first request, via the hint");

		// Again, on the connection the first request established. A client that opened
		// QUIC once and then quietly went back to TCP -- or that failed to reuse the
		// confirmed entry -- fails here while passing above.
		const second = await fetch(`${url}/hello`, { agent, timeout: 15_000 });
		t.equal(await second.text(), PAYLOAD, "and so does the next one");
		t.equal(second.version, "HTTP/3.0", "still over HTTP/3, so the connection is reused");
	},
};
