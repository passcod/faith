/**
 * Apache is configured rather than scripted, and its configuration is the largest
 * in the matrix: a dozen explicit LoadModule lines, three writable paths that must
 * land inside the prefix, and a forced content type without which compression
 * silently does not happen.
 */

const test = require("tape");

const { apacheH1, apacheH2, locateBinary, locateModules } = require("./apache.js");
const { assertKnownCapabilities } = require("../capabilities.js");
const { PAYLOAD, COMPRESSIBLE } = require("../contract.js");
const { assertServesContract } = require("./contract-check.js");

const { Agent } = require("../../../index.js");
const { fetch } = require("../../../wrapper.js");

for (const server of [apacheH1, apacheH2]) {
	test(`apache: ${server.name} serves and negotiates`, async (t) => {
		assertKnownCapabilities(server.capabilities, `server ${server.name}`);

		if (!server.available()) {
			// Naming which half is missing: an installed httpd whose module directory was
			// not found is a different problem from no httpd at all, and the fix differs.
			t.pass(
				`${server.name} is unverified here: binary=${locateBinary() || "none"} ` +
					`modules=${locateModules() || "none"}`,
			);
			t.end();
			return;
		}

		const running = await server.start();
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
			t.equal(
				res.version,
				server.expectVersion,
				"negotiates exactly the protocol the row claims",
			);

			// Compression is the part of this config that fails silently: mod_deflate
			// needs mod_filter loaded and a typed response, and short of either the route
			// is served uncompressed while every status code still looks right.
			const gzipped = await fetch(`${running.url}/encoding/gzip`, { agent, timeout: 10_000 });
			t.equal(await gzipped.text(), COMPRESSIBLE, "the gzip route round-trips");
			t.equal(
				gzipped.headers.get("content-length"),
				null,
				"and was actually compressed, so it carries no Content-Length",
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
