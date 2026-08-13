/**
 * Flow-control windows over HTTP/3 (spec:FLOW).
 *
 * The QUIC windows are carried in transport parameters during the handshake, and
 * Caddy does not report the ones its peer sent, so unlike the HTTP/2 case these
 * cannot assert the advertised values. What they do cover is that the windows
 * Faith sets are ones QUIC accepts and a transfer completes under: the sizes go
 * through quinn's `VarInt`, which rejects out-of-range values, and a connection
 * window smaller than a transfer is exactly the configuration that would stall a
 * body part-way rather than fail loudly.
 *
 * Same topology as the other HTTP/3 tests (see fixtures/h3-blackhole.js).
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
const MIB = 1024 * 1024;
const BODY = "hello-from-caddy";

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

/** Caddy behind the h3 relay, with an agent hinted onto HTTP/3 and built from `options`. */
async function harness(options = {}) {
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
		...options,
		tls: { extraRoots: [ca] },
		// A hint seeds the confirmed state directly, so the first request speaks HTTP/3
		// rather than waiting on a probe.
		http3: { ...(options.http3 ?? {}), hints: [{ host: "localhost", port: front }] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});

	return {
		url: `https://localhost:${front}/`,
		agent,
		async close() {
			agent.close();
			relay.close();
			await tcp.close();
			caddy.close();
		},
	};
}

test("flow control: HTTP/3 transfers on the default windows", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	const h = await harness({});
	try {
		const res = await fetch(h.url, { agent: h.agent, timeout: 15_000 });
		t.equal(res.version, "HTTP/3.0", "negotiated h3");
		t.equal(await res.text(), BODY, "body read in full under the default windows");
	} finally {
		await h.close();
		t.end();
	}
});

test("flow control: the common windows apply to HTTP/3", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	const h = await harness({
		flowControl: { streamWindow: 3 * MIB, connectionWindow: 9 * MIB },
	});
	try {
		const res = await fetch(h.url, { agent: h.agent, timeout: 15_000 });
		t.equal(res.version, "HTTP/3.0", "negotiated h3");
		t.equal(await res.text(), BODY, "body read in full under the common windows");
	} finally {
		await h.close();
		t.end();
	}
});

test("flow control: HTTP/3 windows and send window apply", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	const h = await harness({
		flowControl: { streamWindow: 3 * MIB, connectionWindow: 9 * MIB },
		http3: { streamWindow: 5 * MIB, connectionWindow: 12 * MIB, sendWindow: 2 * MIB },
	});
	try {
		const res = await fetch(h.url, { agent: h.agent, timeout: 15_000 });
		t.equal(res.version, "HTTP/3.0", "negotiated h3");
		t.equal(await res.text(), BODY, "body read in full under the per-protocol windows");
	} finally {
		await h.close();
		t.end();
	}
});

test("flow control: a small window still carries a body larger than it", async (t) => {
	if (!guard(t)) return;

	// A window smaller than the transfer is the case that distinguishes working flow
	// control from none: the origin fills the window, stops, and may only continue once
	// Faith has acknowledged what it read. If the acknowledgements never went out the
	// body would stall here rather than arrive short.
	const { fetch } = require("../wrapper.js");
	const { mkdtempSync, writeFileSync } = require("node:fs");
	const path = require("node:path");

	const STREAM_WINDOW = 16 * 1024;
	const size = 32 * STREAM_WINDOW;
	const dir = mkdtempSync(path.join(os.tmpdir(), "faith-h3-flow-"));
	writeFileSync(path.join(dir, "big"), "x".repeat(size));

	const { ca } = ensureCert();
	const front = await findFreePort();
	const back = await findFreePort();
	const caddy = await startCaddy({
		port: back,
		dir,
		altSvc: `h3=":${front}"`,
		directives: [`root * ${dir}`, "file_server"],
	});
	const tcp = await startTcpProxy({ listenPort: front, upstreamPort: back });
	const relay = await startUdpRelay({ listenPort: front, upstreamPort: back });

	const { Agent } = require("../index.js");
	const agent = new Agent({
		flowControl: { streamWindow: STREAM_WINDOW, connectionWindow: 2 * STREAM_WINDOW },
		tls: { extraRoots: [ca] },
		http3: { hints: [{ host: "localhost", port: front }] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});

	try {
		const res = await fetch(`https://localhost:${front}/big`, { agent, timeout: 15_000 });
		t.equal(res.version, "HTTP/3.0", "negotiated h3");
		t.equal(
			(await res.text()).length,
			size,
			"a body 32 windows long arrives whole, so the window is being replenished",
		);
	} finally {
		agent.close();
		relay.close();
		await tcp.close();
		caddy.close();
		t.end();
	}
});
