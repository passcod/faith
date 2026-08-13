/**
 * Alt-Svc advertisements naming a port other than the origin's.
 *
 * An advertisement names a network endpoint for the origin; it does not claim the
 * origin's own port speaks HTTP/3. Honouring one properly means connecting to
 * that endpoint while still sending the origin's authority, which reqwest cannot
 * express — it derives the HTTP/3 connect target from the request URI's authority
 * (https://github.com/seanmonstar/reqwest/issues/1138). So by default Faith does
 * not upgrade at all on a mismatch, and `http3.upgradeFollowAdvertisedPort` opts
 * into rewriting the request's port instead, which is not standards-compliant.
 *
 * The topology gives us a mismatch for free: Caddy serves on `back` and therefore
 * advertises `h3=":back"`, while the client's origin is `front`.
 *
 *   client ──> https://localhost:FRONT
 *        TCP  127.0.0.1:FRONT ──[tcp proxy]──> 127.0.0.1:BACK
 *        h3   only reachable at 127.0.0.1:BACK, the advertised port
 */

const test = require("tape");
const os = require("node:os");
const {
	ensureCert,
	findFreePort,
	startCaddy,
	startTcpProxy,
	caddyAvailable,
} = require("./fixtures/h3-blackhole.js");

// Linux-only for the same reason as the other HTTP/3 tests: the behaviour is
// client-side, so there's nothing to gain from the whole OS x Node matrix.
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

async function harness(http3Options, { altSvc } = {}) {
	const { ca } = ensureCert();
	const front = await findFreePort();
	const back = await findFreePort();
	const caddy = await startCaddy({ port: back, dir: os.tmpdir(), altSvc });
	const tcp = await startTcpProxy({ listenPort: front, upstreamPort: back });

	const { Agent } = require("../index.js");
	const agent = new Agent({
		tls: { extraRoots: [ca] },
		// No hints: the advertisement Caddy sends is what drives this test.
		http3: http3Options,
		dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
	});

	return {
		front,
		back,
		url: `https://localhost:${front}/`,
		agent,
		async close() {
			await tcp.close();
			caddy.close();
		},
	};
}

test("HTTP/3: an advertisement on another port does not upgrade by default", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	const h = await harness({});

	try {
		// The first response carries Caddy's alt-svc, so the advertisement is in the
		// cache for every attempt after it.
		const first = await fetch(h.url, { agent: h.agent, timeout: 10000 });
		await first.text();
		t.equal(
			first.headers.get("alt-svc"),
			`h3=":${h.back}"; ma=2592000`,
			`precondition: Caddy advertises h3 on :${h.back}, not the origin's :${h.front}`,
		);

		const versions = [];
		for (let i = 0; i < 3; i++) {
			const res = await fetch(h.url, { agent: h.agent, timeout: 10000 });
			await res.text();
			versions.push(res.version);
		}

		t.deepEqual(
			versions,
			["HTTP/2.0", "HTTP/2.0", "HTTP/2.0"],
			"stays on TCP: an advertisement for another port isn't evidence about the origin's",
		);
	} finally {
		await h.close();
		t.end();
	}
});

test("HTTP/3: upgradeFollowAdvertisedPort connects to the advertised port", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");
	// Probing off: this test pins the *inline* upgrade mechanics (the request
	// after the advertisement attempts h3 itself). With probing on, the upgrade
	// lands whenever the background probe finishes, which is a different test —
	// see http3-probe.test.js.
	const h = await harness({ upgradeFollowAdvertisedPort: true, upgradeProbe: false });

	try {
		const first = await fetch(h.url, { agent: h.agent, timeout: 10000 });
		await first.text();
		t.equal(first.version, "HTTP/2.0", "the first request is TCP and learns the advertisement");
		t.equal(
			first.headers.get("alt-svc"),
			`h3=":${h.back}"; ma=2592000`,
			`precondition: the advertised port :${h.back} differs from the origin's :${h.front}`,
		);

		const res = await fetch(h.url, { agent: h.agent, timeout: 10000 });
		await res.text();

		t.equal(res.version, "HTTP/3.0", "the next request reaches HTTP/3 on the advertised port");
		t.equal(
			new URL(res.url).port,
			String(h.back),
			"response.url reports the port actually connected to",
		);
		t.notOk(
			res.redirected,
			"a rewritten port is not a redirect, even though the URL's port changed",
		);
	} finally {
		await h.close();
		t.end();
	}
});

test("HTTP/3: the TCP fallback keeps the origin port, not the rewritten one", async (t) => {
	if (!guard(t)) return;

	const { fetch } = require("../wrapper.js");

	// Advertise a port with nothing listening on it. The h3 attempt must therefore
	// fail, and the fallback can only succeed if it kept the origin's URL. This is
	// what pins the ordering inside the middleware: the request is cloned *before*
	// its port is rewritten, so the clone still targets the origin. Move the rewrite
	// above the clone and this test fails outright — the other tests here can't
	// catch that, because Caddy serves TCP on the advertised port too.
	const dead = await findFreePort();
	// Probing off, as above: the clone-before-rewrite ordering this test pins
	// only exists on the inline path. (With probing on, the dead advertisement
	// would fail in the background and no foreground request would ever carry
	// the rewritten port.)
	const h = await harness(
		{ upgradeFollowAdvertisedPort: true, upgradeProbe: false },
		{ altSvc: `h3=":${dead}"` },
	);

	try {
		const first = await fetch(h.url, { agent: h.agent, timeout: 10000 });
		await first.text();
		t.equal(
			first.headers.get("alt-svc"),
			`h3=":${dead}"`,
			`precondition: h3 is advertised on :${dead}, where nothing is listening`,
		);

		// Catch rather than throw, so breaking the ordering fails with an explanation
		// instead of a bare connect error from deep inside reqwest.
		let res;
		let failure;
		try {
			res = await fetch(h.url, { agent: h.agent, timeout: 10000 });
		} catch (err) {
			failure = err;
		}
		t.ok(
			res,
			failure
				? `expected a TCP fallback to the origin; the request failed instead, which \
means the fallback used the rewritten port (${failure.message})`
				: "the request still succeeds, by falling back to TCP",
		);
		if (!res) return;

		const body = await res.text();
		t.equal(res.version, "HTTP/2.0", "the fallback used TCP");
		t.equal(body, "hello-from-caddy", "and reached the real server");
		t.equal(
			new URL(res.url).port,
			String(h.front),
			"the fallback targeted the origin port, so the clone predates the rewrite",
		);
		t.notOk(res.redirected, "and it isn't reported as a redirect");
	} finally {
		await h.close();
		t.end();
	}
});
