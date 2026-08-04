/**
 * The Alt-Svc layer must sit *inside* the HTTP cache middleware.
 *
 * `http-cache` rebuilds a cached response with the version it was stored with, so
 * a response cached from an HTTP/3 exchange replays as HTTP/3. If the Alt-Svc layer
 * were outside the cache it would see those replays and treat them as live: calling
 * `confirm_h3`, which clears the origin's cancellation strikes and refreshes its
 * confirmed TTL — on evidence that never touched the network.
 *
 * The consequence is that an agent with a cache store can never demote a broken
 * HTTP/3 origin, as long as *something* keeps hitting the cache: every hit wipes the
 * strike run that would otherwise reach the threshold. That neutralises the
 * cancellation-fallback fix for exactly the kind of long-lived, cache-enabled agent
 * that reported it.
 *
 * So this test interleaves cache hits with cancelled requests over a blackholed UDP
 * path. Registered inside the cache, the strikes accumulate and the origin demotes
 * to TCP; registered outside, the hits reset them and it never does.
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

test("HTTP/3: cache hits do not reset cancellation strikes", async (t) => {
	if (!SUPPORTED) {
		t.pass(`skipped on ${process.platform} (linux-only harness)`);
		t.end();
		return;
	}
	if (!caddyAvailable()) {
		if (process.env.CI) {
			t.fail("caddy is not on PATH but CI is set; the install step must provide it");
		} else {
			t.pass("skipped: caddy not on PATH (install it to run this test locally)");
		}
		t.end();
		return;
	}

	const { Agent } = require("../index.js");
	const { fetch } = require("../wrapper.js");
	const { ca } = ensureCert();

	const front = await findFreePort();
	const back = await findFreePort();
	const caddy = await startCaddy({
		port: back,
		dir: os.tmpdir(),
		cacheControl: "public, max-age=300",
	});
	const tcp = await startTcpProxy({ listenPort: front, upstreamPort: back });
	const relay = await startUdpRelay({ listenPort: front, upstreamPort: back });

	const agent = new Agent({
		tls: { extraRoots: [ca] },
		cache: { store: "memory" },
		http3: { hints: [{ host: "localhost", port: front }] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});
	const base = `https://localhost:${front}`;

	const attempt = async (path, opts) => {
		try {
			const res = await fetch(`${base}${path}`, { agent, ...opts });
			await res.text();
			return { ok: true, version: res.version };
		} catch (err) {
			return { ok: false, code: err.code };
		}
	};

	try {
		// Warm up: confirm HTTP/3 for the origin, and populate the cache for /cached.
		let warm;
		for (let i = 0; i < 2; i++) warm = await attempt("/cached", { timeout: 10000 });
		t.equal(warm.version, "HTTP/3.0", "precondition: the origin is confirmed as HTTP/3");

		const cachedAgain = await attempt("/cached", { timeout: 10000 });
		t.ok(cachedAgain.ok, "precondition: /cached is served again (from the cache)");

		// Break only the UDP path. Caddy's TCP listener stays healthy throughout.
		relay.blackhole();

		// Interleave: a cache hit, then a cancelled request to a distinct path so it
		// must go to the network. Four rounds is comfortably past the default strike
		// threshold of 3, so a working demotion happens well inside the loop.
		const cancelled = [];
		for (let i = 0; i < 4; i++) {
			await attempt("/cached", { timeout: 10000 });
			cancelled.push(await attempt(`/live-${i}`, { signal: AbortSignal.timeout(1200) }));
		}

		const recovered = cancelled.filter((r) => r.ok);
		t.ok(
			recovered.length > 0,
			"strikes survive the interleaved cache hits, so the origin demotes to TCP",
		);
		t.ok(
			recovered.some((r) => r.version === "HTTP/2.0"),
			"and the recovered request used TCP",
		);
	} finally {
		relay.close();
		await tcp.close();
		caddy.close();
		t.end();
	}
});
