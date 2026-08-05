/**
 * quiche is the only row with no TCP listener, so it is the only one where the
 * agent's seeded hint is load-bearing: without it a request goes to a port where
 * nothing answers over TCP, and the row looks broken rather than misconfigured.
 */

const test = require("tape");

const { quiche, locateBinary } = require("./quiche.js");
const { assertKnownCapabilities } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { assertServesContract } = require("./contract-check.js");

const { Agent } = require("../../../index.js");
const { fetch } = require("../../../wrapper.js");

test("quiche: serves over HTTP/3 and nothing else", async (t) => {
	assertKnownCapabilities(quiche.capabilities, "server quiche");

	if (!quiche.available()) {
		t.pass("quiche-server is not built here, so its configuration is unverified");
		t.end();
		return;
	}

	const running = await quiche.start();
	const agent = new Agent({
		tls: { extraRoots: [running.ca] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
		// The row's own options, applied here the way the runner applies them.
		...running.agentOptions,
	});

	try {
		const res = await fetch(`${running.url}/hello`, { agent, timeout: 15_000 });
		const body = await res.text();
		t.equal(res.status, 200, "serves the baseline route");
		t.equal(body, PAYLOAD, "and the expected payload");
		t.equal(res.version, "HTTP/3.0", "over HTTP/3, which is all it speaks");

		await assertServesContract(t, {
			url: running.url,
			agent,
			capabilities: quiche.capabilities,
		});
	} finally {
		await running.close();
		t.end();
	}
});

test("quiche: locating the binary says how", (t) => {
	// The failure mode worth naming: no package installs this, so an absent binary
	// means the build step did not run rather than that a server is missing.
	t.equal(
		typeof locateBinary(),
		quiche.available() ? "string" : "object",
		"either a usable command name or null, never a path that does not run",
	);
	t.end();
});
