/**
 * `preconnect` over HTTP/3: a warm-up establishes the connection over the transport the next
 * foreground request would take, so it means the same thing whichever protocol the origin
 * lands on (spec:WARM#preconnect).
 *
 * A confirmed origin is the HTTP/3 case, and a hint is the cheapest way to reach it: a hint is
 * the user's own assertion, so it seeds the confirmed state directly rather than going through a
 * probe. Same topology as the other HTTP/3 tests (see fixtures/h3-blackhole.js).
 */

const test = require("tape");
const os = require("node:os");
const {
	ensureCert,
	findFreePort,
	startCaddy,
	startTcpProxy,
	startUdpRelay,
	caddyAvailable,
} = require("./fixtures/h3-blackhole.js");

const SUPPORTED = process.platform === "linux";

function guard(t) {
	if (!SUPPORTED) {
		t.pass(`skipped on ${process.platform} (linux-only harness)`);
		t.end();
		return false;
	}
	if (!caddyAvailable()) {
		if (process.env.CI) {
			t.fail("caddy is not on PATH but CI is set; the install step must provide it");
		} else {
			t.pass("skipped: caddy not on PATH (install it to run this test locally)");
		}
		t.end();
		return false;
	}
	return true;
}

async function harness(http3Options = {}) {
	const { ca } = ensureCert();
	const front = await findFreePort();
	const back = await findFreePort();
	const caddy = await startCaddy({
		port: back,
		dir: os.tmpdir(),
		altSvc: `h3=":${front}"`,
	});
	const tcp = await startTcpProxy({ listenPort: front, upstreamPort: back });
	const relay = await startUdpRelay({ listenPort: front, upstreamPort: back });

	const { Agent } = require("../index.js");
	const agent = new Agent({
		tls: { extraRoots: [ca] },
		http3: http3Options,
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});

	return {
		front,
		url: `https://localhost:${front}/`,
		origin: `https://localhost:${front}`,
		agent,
		relay,
		async close() {
			relay.close();
			await tcp.close();
			await caddy.close();
		},
	};
}

test("preconnect warms a confirmed HTTP/3 origin over QUIC", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	const h = await harness({});
	// Re-create the agent with a hint for the port the harness picked, so the origin starts
	// confirmed and the warm-up takes the QUIC path a foreground request would.
	const { Agent } = require("../index.js");
	const { ca } = ensureCert();
	h.agent = new Agent({
		tls: { extraRoots: [ca] },
		http3: { hints: [{ host: "localhost", port: h.front }] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});

	try {
		await h.agent.preconnect(h.origin);

		// QUIC connections are not tracked, so a warm-up that took the h3 path leaves nothing
		// in `connections()` — that absence is what distinguishes it from the TCP path.
		t.equal(
			h.agent.connections().length,
			0,
			"a QUIC warm-up is not listed among TCP connections (spec:OBS)",
		);

		const response = await fetch(h.url, { agent: h.agent, timeout: 15_000 });
		await response.text();
		t.equal(
			response.version,
			"HTTP/3.0",
			"the foreground request runs over the warmed QUIC connection",
		);
	} finally {
		await h.close();
		t.end();
	}
});

test("preconnect warms an unconfirmed origin over TCP", async (t) => {
	if (!guard(t)) return;

	const h = await harness({});

	try {
		// No hint and nothing advertised yet, so the origin is unconfirmed: foreground requests
		// upgrade only from the confirmed state, so the warm-up takes TCP. The contrast with the
		// hinted case above is the whole point — a TCP warm-up is listed, a QUIC one is not.
		await h.agent.preconnect(h.origin);

		const conns = h.agent.connections();
		t.equal(conns.length, 1, "the TCP warm-up connection is listed");
		t.equal(conns[0].responseCount, 0, "at a response count of zero");
	} finally {
		await h.close();
		t.end();
	}
});

test("preconnect routes the way the agent's upgrade settings would route a request", async (t) => {
	if (!guard(t)) return;

	const { ensureCert } = require("./fixtures/h3-blackhole.js");
	const { Agent } = require("../index.js");
	const h = await harness({});
	const { ca } = ensureCert();

	try {
		// With the upgrade machinery off, nothing routes to QUIC whatever the caches hold — so
		// even a hinted origin is warmed over TCP, and the connection is listed.
		const disabled = new Agent({
			tls: { extraRoots: [ca] },
			http3: {
				upgradeEnabled: false,
				hints: [{ host: "localhost", port: h.front }],
			},
			dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
		});

		await disabled.preconnect(h.origin);
		t.equal(
			disabled.connections().length,
			1,
			"upgradeEnabled: false warms over TCP despite the hint",
		);

		// With probing off, the legacy inline upgrade acts on a hint as the foreground path
		// would, so the warm-up takes QUIC and leaves nothing among the TCP connections.
		const inline = new Agent({
			tls: { extraRoots: [ca] },
			http3: {
				upgradeProbe: false,
				hints: [{ host: "localhost", port: h.front }],
			},
			dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
		});

		await inline.preconnect(h.origin);
		t.equal(
			inline.connections().length,
			0,
			"upgradeProbe: false warms over QUIC, matching the inline upgrade",
		);
	} finally {
		await h.close();
		t.end();
	}
});
