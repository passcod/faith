/**
 * nginx is configured rather than scripted, and its configuration is the most
 * fragile thing in this row: every writable path has to be inside the prefix, and
 * how HTTP/2 is spelled depends on the version. Both fail in ways that look like
 * client bugs from inside a dimension.
 */

const test = require("tape");

const { nginx, parseHttp2Style } = require("./nginx.js");
const { assertKnownCapabilities } = require("../capabilities.js");
const { PAYLOAD } = require("../contract.js");
const { assertServesContract } = require("./contract-check.js");

const { Agent } = require("../../../index.js");
const { fetch } = require("../../../wrapper.js");

// Only one nginx is installed on any given machine, so the spelling this picks for
// the *other* version is never exercised by starting a server. Getting it wrong
// leaves nginx quietly serving HTTP/1.1, which the row's version probe reports as
// though faith had failed to negotiate -- so the boundary is checked directly.
test("nginx: HTTP/2 is spelled the way this version wants it", (t) => {
	const legacy = parseHttp2Style("nginx version: nginx/1.24.0");
	t.equal(legacy.listenSuffix, " http2", "1.24 takes the listen parameter");
	t.equal(legacy.directive, "", "and not the standalone directive");

	// 1.25.1 is the release that deprecated the listen parameter, so it is the first
	// version on the modern side rather than a round number.
	const boundary = parseHttp2Style("nginx version: nginx/1.25.1");
	t.equal(boundary.directive, "http2 on;", "1.25.1 takes the standalone directive");
	t.equal(boundary.listenSuffix, "", "and not the listen parameter");

	const before = parseHttp2Style("nginx version: nginx/1.25.0");
	t.equal(before.listenSuffix, " http2", "1.25.0, one patch earlier, is still legacy");

	const unreadable = parseHttp2Style("something else entirely");
	t.equal(unreadable.directive, "http2 on;", "an unreadable banner guesses forward");
	t.equal(unreadable.version, null, "and says it could not tell");

	t.end();
});

test("nginx: serves and negotiates", async (t) => {
	assertKnownCapabilities(nginx.capabilities, "server nginx");

	if (!nginx.available()) {
		t.pass("nginx is not installed, so its configuration is unverified");
		t.end();
		return;
	}

	const running = await nginx.start();
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
		t.equal(res.version, nginx.expectVersion, "negotiates exactly the protocol the row claims");

		await assertServesContract(t, {
			url: running.url,
			agent,
			capabilities: nginx.capabilities,
		});
	} finally {
		await running.close();
		t.end();
	}
});
