/**
 * Content coding: does the client actually decompress what the server labelled?
 *
 * The awkward part is that a server which declined to compress produces a response
 * this dimension's obvious assertions accept: the body round-trips, and no
 * Content-Encoding survives because none was ever sent. Every configured server
 * declines below some size threshold -- 20 bytes for mod_deflate, 512 for Caddy --
 * so the third assertion is the one that makes the other two mean anything. It
 * compares what arrived on the wire against what came out of the decoder.
 */

const { CAPABILITIES: C } = require("../capabilities.js");
const { COMPRESSIBLE } = require("../contract.js");
const { fetch } = require("../../../wrapper.js");

module.exports = {
	name: "gzip",
	requires: [C.GZIP],
	// Declared so the runner catches this dimension quietly losing its coverage:
	// tape counts a test that asserts nothing as a pass.
	assertions: 4,
	negativeAssertions: 2,

	async run(t, { url, agent }) {
		const res = await fetchWith(agent, `${url}/encoding/gzip`);
		// Read the wire length before the body: once the decoder runs, this header is
		// the only trace left of what actually arrived.
		const encodedLength = res.headers.get("content-length");
		const body = await res.text();

		t.equal(body, COMPRESSIBLE, "transparently decompresses gzip");
		t.notOk(
			res.headers.get("content-encoding"),
			"and strips Content-Encoding once decoded, so the body matches the header",
		);

		// The control: ask for no encoding at all. A server that reports the full size
		// here can report a size, so the encoded response arriving without one -- or
		// with a smaller one -- is a difference the server made, not an accident of how
		// it frames responses.
		const plain = await fetch(`${url}/encoding/gzip`, {
			agent,
			timeout: 10_000,
			headers: { "accept-encoding": "identity" },
		});
		const plainLength = plain.headers.get("content-length");
		await plain.text();
		t.equal(
			plainLength,
			String(Buffer.byteLength(COMPRESSIBLE)),
			"asked for identity, the server sends the whole body and says how long it is",
		);
		t.ok(
			encodedLength === null || Number(encodedLength) < Number(plainLength),
			`so asking for gzip got something else: ${encodedLength ?? "chunked"} on the ` +
				`wire against ${plainLength} for identity`,
		);
	},

	async negative(t, { url, agent }) {
		// Labelled gzip, sent as plain text. A client that really decompresses must
		// fail; one that passes the bytes through would happily return them.
		//
		// Split the fetch from the body read, because that is what makes this
		// discriminating. Decoding happens while streaming the body, so a
		// mislabelled body yields perfectly good headers and fails only on read --
		// whereas a connect, TLS or timeout failure would reject the fetch itself.
		// Wrapping both in one try/catch would let any of those satisfy the
		// assertion. faith's body-stream errors carry no `code` property, so this
		// separation is the discriminator available to us, not error matching.
		const res = await fetchWith(agent, `${url}/encoding/mislabelled`);
		t.equal(
			res.status,
			200,
			"the response itself arrives -- so a later failure is about the body, not the connection",
		);

		let failed = false;
		try {
			await res.text();
		} catch {
			failed = true;
		}
		t.ok(
			failed,
			"reading a body mislabelled as gzip fails rather than yielding the raw bytes",
		);
	},
};

function fetchWith(agent, target) {
	return fetch(target, { agent, timeout: 10000 });
}
