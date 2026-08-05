/**
 * The controllable origin is test infrastructure, so it needs its own test:
 * every dimension's verdict depends on it serving what it claims.
 */

const test = require("tape");
const { controllableH1, controllableH2 } = require("./controllable.js");
const { assertKnownCapabilities } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { assertServesContract } = require("./contract-check.js");

const { Agent } = require("../../../index.js");
const { fetch } = require("../../../wrapper.js");

for (const server of [controllableH1, controllableH2]) {
	test(`controllable origin: ${server.name} serves and negotiates`, async (t) => {
		assertKnownCapabilities(server.capabilities, `server ${server.name}`);

		const running = await server.start();
		const agent = new Agent({
			tls: { extraRoots: [running.ca] },
			dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
			http3: { upgradeEnabled: false },
		});

		try {
			const res = await fetch(`${running.url}/hello`, { agent, timeout: 10000 });
			const body = await res.text();
			t.equal(res.status, 200, "serves the baseline route");
			t.equal(body, PAYLOAD, "and the expected payload");
			t.equal(
				res.version,
				server.expectVersion,
				"negotiates exactly the protocol the row claims",
			);

			await assertServesContract(t, {
				url: running.url,
				agent,
				capabilities: server.capabilities,
			});
		} finally {
			await running.close();
			t.end();
		}
	});
}
