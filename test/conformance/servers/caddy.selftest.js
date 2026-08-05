/**
 * Caddy is configured rather than scripted, so its selftest is where a Caddyfile
 * mistake surfaces. Without it, a directive that silently stopped matching would
 * show up as a dimension failing on a 404 body.
 */

const test = require("tape");

const { caddy } = require("./caddy.js");
const { assertKnownCapabilities } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { assertServesContract } = require("./contract-check.js");

const { Agent } = require("../../../index.js");
const { fetch } = require("../../../wrapper.js");

test(`caddy: serves and negotiates`, async (t) => {
	assertKnownCapabilities(caddy.capabilities, "server caddy");

	if (!caddy.available()) {
		// Same rule as the runner: absence is reported, never silent. The row's cells
		// carry the CONFORMANCE_REQUIRE_ALL decision, so this does not repeat it.
		t.pass("caddy is not installed, so its configuration is unverified");
		t.end();
		return;
	}

	const running = await caddy.start();
	const agent = new Agent({
		tls: { extraRoots: [running.ca] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
		http3: { upgradeEnabled: false },
	});

	try {
		const res = await fetch(`${running.url}/hello`, { agent, timeout: 10_000 });
		const body = await res.text();
		t.equal(res.status, 200, "serves the baseline route");
		t.equal(body, PAYLOAD, "and the expected payload");
		t.equal(res.version, caddy.expectVersion, "negotiates exactly the protocol the row claims");
		t.ok(res.headers.get("alt-svc"), "advertises Alt-Svc, which is why this row is here");

		await assertServesContract(t, {
			url: running.url,
			agent,
			capabilities: caddy.capabilities,
		});
	} finally {
		await running.close();
		t.end();
	}
});
