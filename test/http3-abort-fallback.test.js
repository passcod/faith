/**
 * Regression test: a cancelled HTTP/3 attempt must still demote the origin and
 * fall back to TCP.
 *
 * AltSvcMiddleware only records an HTTP/3 failure on the `Err` arm of the h3
 * attempt. fetch() races request.send() against the abort signal in a select!,
 * so an abort *drops* that future and neither arm runs: record_h3_failure is
 * unreachable under cancellation. The confirmed Alt-Svc entry then survives
 * (24h TTL) and every retry re-attempts h3 over a dead UDP path, forever,
 * while TCP is healthy the whole time.
 *
 * The test asserts the behaviour we want, so it FAILS until that is fixed.
 *
 * Timing note: this deliberately finishes well inside quinn's 30s idle timeout.
 * Once that timer fires, the h3 driver dies and SendRequest starts erroring
 * immediately, which *can* let a request escape and record the failure. That
 * escape is a race — it only happens if a request is in flight at that exact
 * moment — and relying on it either way would make this test flaky.
 */

const test = require("tape");
const path = require("node:path");
const os = require("node:os");
const {
	ensureCert,
	findFreePort,
	startCaddy,
	startTcpProxy,
	startUdpRelay,
	caddyAvailable,
} = require("./fixtures/h3-blackhole.js");

// The harness needs a userspace UDP relay on a fixed loopback port and a Caddy
// binary. Restricted to Linux: the bug is entirely client-side and platform
// independent, so there is nothing to gain from paying for it on every job of
// the OS x Node matrix.
const SUPPORTED = process.platform === "linux";

test("HTTP/3: a cancelled h3 attempt falls back to TCP", async (t) => {
	if (!SUPPORTED) {
		t.pass(`skipped on ${process.platform} (linux-only harness)`);
		t.end();
		return;
	}
	if (!caddyAvailable()) {
		// Never let CI silently skip this: if the install step regressed, fail.
		if (process.env.CI) {
			t.fail("caddy is not on PATH but CI is set; the install step must provide it");
			t.end();
			return;
		}
		t.pass("skipped: caddy not on PATH (install it to run this test locally)");
		t.end();
		return;
	}

	const { Agent } = require("../index.js");
	const { fetch } = require("../wrapper.js");
	const { ca } = ensureCert();

	const front = await findFreePort();
	const back = await findFreePort();
	const caddy = await startCaddy({ port: back, dir: os.tmpdir() });
	const tcp = await startTcpProxy({ listenPort: front, upstreamPort: back });
	const relay = await startUdpRelay({ listenPort: front, upstreamPort: back });

	const agent = new Agent({
		tls: { extraRoots: [ca] },
		// Pin the origin to the relay/proxy port. A hint rather than Caddy's own
		// Alt-Svc header so the test does not depend on which port faith decides
		// to attempt h3 on (see the separate advertised-port issue).
		http3: { hints: [{ host: "localhost", port: front }] },
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});
	const url = `https://localhost:${front}/`;

	const attempt = async (opts) => {
		try {
			const res = await fetch(url, { agent, ...opts });
			await res.text();
			return { ok: true, version: res.version };
		} catch (err) {
			return { ok: false, code: err.code };
		}
	};

	try {
		// Warm up until HTTP/3 is confirmed for the origin.
		let warm;
		for (let i = 0; i < 3; i++) warm = await attempt({ timeout: 10000 });
		t.equal(
			warm.version,
			"HTTP/3.0",
			"precondition: the origin is confirmed as HTTP/3 through the relay",
		);

		// Break only the UDP path. Caddy's TCP listener stays healthy.
		relay.blackhole();

		// A retry loop that cancels via AbortSignal, as a caller with its own
		// deadline would. Four attempts is plenty: a working fallback demotes the
		// origin on the very first failure.
		const results = [];
		for (let i = 0; i < 4; i++) {
			results.push(await attempt({ signal: AbortSignal.timeout(1500) }));
		}

		const succeeded = results.filter((r) => r.ok);
		t.ok(
			succeeded.length > 0,
			"at least one retry falls back to TCP while UDP is blackholed",
		);
		t.ok(
			succeeded.some((r) => r.version === "HTTP/2.0"),
			"the fallback actually used TCP (HTTP/2)",
		);
	} finally {
		relay.close();
		await tcp.close();
		caddy.close();
		t.end();
	}
});
