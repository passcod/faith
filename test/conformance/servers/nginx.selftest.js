/**
 * nginx is configured rather than scripted, and its configuration is the most
 * fragile thing in this row: every writable path has to be inside the prefix, and
 * how HTTP/2 is spelled depends on the version. Both fail in ways that look like
 * client bugs from inside a dimension.
 */

const test = require("tape");

const { nginx, parseVersion, newEnough, MINIMUM } = require("./nginx.js");
const { assertKnownCapabilities } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { assertServesContract } = require("./contract-check.js");

const { Agent } = require("../../../index.js");
const { fetch } = require("../../../wrapper.js");

// Only one nginx is installed on any given machine, so the floor this enforces is
// never exercised from both sides by starting a server. It matters because the two
// nginx boundaries land three weeks apart: HTTP/3 in 1.25.0, and the standalone
// `http2` directive in 1.25.1. The config speaks only the newer dialect, so anything
// between them would be handed a directive it does not know and refuse to start.
test("nginx: the version floor sits above both boundaries", (t) => {
	t.notOk(newEnough(parseVersion("nginx version: nginx/1.24.0")), "1.24 has no HTTP/3 at all");
	t.notOk(
		newEnough(parseVersion("nginx version: nginx/1.25.0")),
		"1.25.0 has HTTP/3 but wants the deprecated listen parameter, so it is out too",
	);
	t.ok(
		newEnough(parseVersion("nginx version: nginx/1.25.1")),
		`1.25.1 is the floor, and takes the http2 directive this config writes`,
	);
	t.ok(newEnough(parseVersion("nginx version: nginx/1.31.3")), "and anything newer is fine");
	t.ok(newEnough(parseVersion("nginx version: nginx/2.0.0")), "including a major bump");

	// An unreadable banner is not new enough: the row would rather report itself
	// unavailable, with the version it could not read, than hand a config to a binary
	// it knows nothing about.
	t.notOk(newEnough(parseVersion("something else entirely")), "an unreadable banner is not");
	t.equal(MINIMUM.join("."), "1.25.1", "and the floor is stated once, where it is enforced");

	t.end();
});

for (const server of [nginx]) {
	test(`nginx: ${server.name} serves and negotiates`, async (t) => {
		assertKnownCapabilities(server.capabilities, `server ${server.name}`);

		if (!server.available()) {
			// For the h3 row this is usually a build fact rather than a missing server:
			// HTTP/3 is a compile-time module, and no Ubuntu 24.04 package has it.
			t.pass(`${server.name} is unavailable here, so its configuration is unverified`);
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

			// nginx writes no advertisement of its own, so on the h3 row this header is
			// entirely the config's doing -- and it is what the altsvc dimension reads.
			if (server.capabilities.has("altsvc")) {
				t.ok(
					res.headers.get("alt-svc"),
					"and carries the hand-written Alt-Svc header, which nginx never adds itself",
				);
			}

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
