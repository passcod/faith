/**
 * The background HTTP/3 probe: no foreground request ever waits on an
 * unverified QUIC path.
 *
 * An Alt-Svc advertisement says the server listens on UDP; it cannot say there
 * is UDP connectivity between here and there. Before the probe, the first
 * request after an advertisement attempted HTTP/3 inline, and on a silently
 * broken UDP path it stalled until the QUIC idle timeout (~30s) or
 * `upgradeAttemptTimeout` before falling back — recurring every
 * `upgradeFailedTtl`, sacrificing a random foreground request each cycle.
 *
 * With the probe (the default), requests keep to TCP until a background
 * `HEAD /` over HTTP/3 confirms the path. These tests pin both halves of that
 * contract: a broken path costs zero foreground latency, and a healthy path
 * still upgrades — plus the recovery loop between them.
 *
 * Same topology as the other HTTP/3 tests (see fixtures/h3-blackhole.js), with
 * Caddy told to advertise the *front* port so the advertisement is actionable
 * for the origin the client sees.
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

/**
 * Generous for a loopback request, tiny next to the ~30s QUIC idle timeout or
 * the 60s default `upgradeAttemptTimeout` a stalled inline attempt would eat.
 * A foreground request slower than this means something waited on HTTP/3.
 */
const FOREGROUND_BUDGET = 2_000;

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
		// Advertise the front port: that is the origin's own port, so the
		// advertisement is actionable without upgradeFollowAdvertisedPort, and
		// the UDP relay makes (or breaks) the path to it.
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
		agent,
		relay,
		async close() {
			relay.close();
			await tcp.close();
			caddy.close();
		},
	};
}

/** One request, timed, body consumed. */
async function timed(fetch, url, agent) {
	const t0 = Date.now();
	const res = await fetch(url, { agent, timeout: 15_000 });
	await res.text();
	return { version: res.version, elapsed: Date.now() - t0, altSvc: res.headers.get("alt-svc") };
}

test("HTTP/3 probe: a blackholed UDP path never stalls a foreground request", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	const h = await harness();

	// Broken from the very start: the advertisement will arrive over TCP, the
	// probe will fire into the void, and no foreground request may notice.
	h.relay.blackhole();

	try {
		const first = await timed(fetch, h.url, h.agent);
		t.ok(first.altSvc, "precondition: the first (TCP) response advertises HTTP/3");

		// Enough requests to have straddled the probe's whole lifetime (5s
		// default timeout) — where the inline upgrade would have sacrificed one
		// of these to a 30-60s stall.
		const results = [];
		for (let i = 0; i < 4; i++) {
			results.push(await timed(fetch, h.url, h.agent));
			await new Promise((r) => setTimeout(r, 100));
		}

		t.ok(
			results.every((r) => r.version !== "HTTP/3.0"),
			"every request stayed on TCP: the unverified path was never routed to",
		);
		const slowest = Math.max(...results.map((r) => r.elapsed));
		t.ok(
			slowest < FOREGROUND_BUDGET,
			`no request waited on the broken path (slowest: ${slowest}ms)`,
		);
	} finally {
		await h.close();
		t.end();
	}
});

test("HTTP/3 probe: a healthy path upgrades in the background", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	const h = await harness();

	try {
		const first = await timed(fetch, h.url, h.agent);
		t.equal(first.version, "HTTP/2.0", "the first request is TCP and learns the advertisement");

		// The upgrade lands whenever the probe's handshake completes, not on a
		// fixed request count: poll until it does.
		let upgraded;
		const deadline = Date.now() + 10_000;
		do {
			upgraded = await timed(fetch, h.url, h.agent);
			if (upgraded.version === "HTTP/3.0") break;
			await new Promise((r) => setTimeout(r, 100));
		} while (Date.now() < deadline);

		t.equal(upgraded.version, "HTTP/3.0", "a later request rides the probed path");
		t.ok(
			upgraded.elapsed < FOREGROUND_BUDGET,
			`the upgraded request was served from a warm connection, not a fresh handshake \
(${upgraded.elapsed}ms)`,
		);
	} finally {
		await h.close();
		t.end();
	}
});

test("HTTP/3 probe: recovery re-enters through a probe, not a foreground gamble", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	// A short failed TTL so the recovery cycle fits in a test; everything else
	// stays at defaults.
	const h = await harness({ upgradeFailedTtl: 1 });

	h.relay.blackhole();

	try {
		// Learn the advertisement over TCP and let the probe fail in the
		// background. Foreground requests stay fast on TCP throughout.
		const first = await timed(fetch, h.url, h.agent);
		t.ok(first.altSvc, "precondition: the advertisement arrived over TCP");

		// Outwait the probe (5s default timeout) plus the 1s failed TTL, with
		// TCP requests interleaved to show none of them pays for any of it.
		const during = [];
		const settle = Date.now() + 6_500;
		while (Date.now() < settle) {
			during.push(await timed(fetch, h.url, h.agent));
			await new Promise((r) => setTimeout(r, 500));
		}
		t.ok(
			during.every((r) => r.version !== "HTTP/3.0" && r.elapsed < FOREGROUND_BUDGET),
			"while broken and through the failure's expiry, TCP requests stay fast",
		);

		// Heal the path. The next response's advertisement re-arms a probe, and
		// the origin comes back to HTTP/3 — still without a stalled request.
		h.relay.restore();

		let recovered;
		const deadline = Date.now() + 15_000;
		do {
			recovered = await timed(fetch, h.url, h.agent);
			if (recovered.version === "HTTP/3.0") break;
			await new Promise((r) => setTimeout(r, 200));
		} while (Date.now() < deadline);

		t.equal(recovered.version, "HTTP/3.0", "the healed path is re-probed and re-adopted");
		t.ok(
			recovered.elapsed < FOREGROUND_BUDGET,
			`recovery cost no foreground stall either (${recovered.elapsed}ms)`,
		);
	} finally {
		await h.close();
		t.end();
	}
});
