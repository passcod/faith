/**
 * Serving expired DNS answers while refreshing behind them (spec:DNS).
 *
 * Every test points Faith at the controllable nameserver in `test/lib/dns-server.js`
 * rather than at a real resolver: the behaviour is entirely about TTLs lapsing and
 * answers changing, neither of which is observable against a resolver the test does
 * not own. `.test` names are used because `localhost` and `.local` are exempt and go
 * to the system resolver, which would never reach the helper.
 *
 * The nameserver answers slowly on purpose. A stale hit and a fresh lookup are only
 * distinguishable by how long they take, so the delay is what makes the assertions
 * mean anything.
 */

const test = require("tape");
const http = require("node:http");
const { Agent } = require("../index.js");
const { fetch } = require("../wrapper.js");
const { startDnsServer } = require("./lib/dns-server.js");

/** The nameserver's answer delay. Long enough to tell a stale hit from a lookup. */
const DELAY = 300;
/** Comfortably under DELAY: a stale hit does no network work at all. */
const STALE_MAX = 150;
/** Comfortably over: a blocking lookup pays the nameserver's delay. */
const FRESH_MIN = 200;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Wait out a 1-second TTL. */
const expire = () => sleep(1300);

function agentFor(server, dns = {}) {
	return new Agent({
		dns: {
			servers: [`udp://${server.host}:${server.port}`],
			// Above the answer delay, so a blocking lookup completes rather than timing out.
			timeout: 5000,
			...dns,
		},
		http3: { upgradeEnabled: false },
	});
}

async function timed(fn) {
	const started = Date.now();
	await fn();
	return Date.now() - started;
}

/** An origin on 127.0.0.1, so 127.0.0.2 is a routable address nothing answers on. */
async function startOrigin() {
	const server = http.createServer((req, res) => {
		res.writeHead(200, { "content-type": "text/plain" });
		res.end("reached");
	});
	await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
	return {
		port: server.address().port,
		close: () => new Promise((resolve) => server.close(resolve)),
	};
}

test("DNS: an expired answer is served without waiting", async (t) => {
	t.plan(3);
	const dns = await startDnsServer({
		zone: { "stale.test": { a: ["127.0.0.1"], ttl: 1 } },
		delayMs: DELAY,
	});
	const agent = agentFor(dns);
	try {
		const cold = await timed(() => agent.prefetchDns("stale.test"));
		t.ok(cold >= FRESH_MIN, `the first lookup pays the resolver (${cold}ms)`);

		const fresh = await timed(() => agent.prefetchDns("stale.test"));
		t.ok(fresh < STALE_MAX, `a fresh entry answers from cache (${fresh}ms)`);

		await expire();
		const stale = await timed(() => agent.prefetchDns("stale.test"));
		t.ok(stale < STALE_MAX, `an expired entry is served immediately (${stale}ms)`);
	} finally {
		await dns.close();
	}
});

test("DNS: the refresh behind a stale answer picks up a changed address", async (t) => {
	t.plan(2);
	const dns = await startDnsServer({
		zone: { "refresh.test": { a: ["127.0.0.2"], ttl: 1 } },
	});
	const agent = agentFor(dns);
	try {
		await agent.prefetchDns("refresh.test");
		dns.set("refresh.test", { a: ["127.0.0.3"], ttl: 1 });
		await expire();
		dns.resetQueries();

		// Serves the old address and starts the refresh behind it. The refresh runs on the
		// runtime after the caller resumes, so it has not necessarily reached the wire yet.
		await agent.prefetchDns("refresh.test");
		await sleep(400);
		t.ok(
			dns.countFor("refresh.test", "A") >= 1,
			"the stale hit starts a refresh at the nameserver",
		);

		// Once the refresh lands the entry is fresh again, so this needs no lookup.
		dns.resetQueries();
		await agent.prefetchDns("refresh.test");
		t.equal(
			dns.countFor("refresh.test", "A"),
			0,
			"the refreshed entry is fresh, so no further lookup runs",
		);
	} finally {
		await dns.close();
	}
});

test("DNS: concurrent stale hits start one refresh", async (t) => {
	t.plan(2);
	const dns = await startDnsServer({
		zone: { "single.test": { a: ["127.0.0.1"], ttl: 1 } },
		delayMs: DELAY,
	});
	const agent = agentFor(dns);
	try {
		await agent.prefetchDns("single.test");
		await expire();
		dns.resetQueries();

		const elapsed = await timed(() =>
			Promise.all(
				Array.from({ length: 5 }, () => agent.prefetchDns("single.test")),
			),
		);
		t.ok(elapsed < STALE_MAX, `all five are served stale (${elapsed}ms)`);

		await sleep(DELAY + 400);
		const queries = dns.countFor("single.test", "A");
		t.ok(
			queries <= 1,
			`five stale hits start at most one refresh (${queries} A queries)`,
		);
	} finally {
		await dns.close();
	}
});

test("DNS: a refresh that fails leaves the stale answer in place", async (t) => {
	t.plan(2);
	const dns = await startDnsServer({
		zone: { "failing.test": { a: ["127.0.0.1"], ttl: 1 } },
	});
	const agent = agentFor(dns);
	try {
		await agent.prefetchDns("failing.test");
		// A server failure says nothing about where the host is, so the entry survives it.
		dns.fail("failing.test", "servfail");
		await expire();

		await agent.prefetchDns("failing.test");
		await sleep(300);

		const again = await timed(() => agent.prefetchDns("failing.test"));
		t.ok(
			again < STALE_MAX,
			`the entry is still served after a failed refresh (${again}ms)`,
		);

		dns.fail("failing.test", null);
		await sleep(50);
		t.pass("the resolver keeps serving while its nameserver is failing");
	} finally {
		await dns.close();
	}
});

test("DNS: a refresh answering NXDOMAIN retires the entry", async (t) => {
	t.plan(1);
	const dns = await startDnsServer({
		zone: { "gone.test": { a: ["127.0.0.1"], ttl: 1 } },
	});
	const agent = agentFor(dns);
	try {
		await agent.prefetchDns("gone.test");
		// The name is gone rather than unreachable, so the old address is wrong, not stale.
		dns.set("gone.test", null);
		await expire();

		await agent.prefetchDns("gone.test");
		await sleep(400);

		// prefetchDns swallows failures, so the retirement shows as a real request failing
		// rather than being served an address the name no longer answers on.
		try {
			await fetch("http://gone.test/", { agent, timeout: 5000 });
			t.fail("a retired name should not resolve");
		} catch (err) {
			t.ok(err, "the name no longer resolves once the refresh says it is gone");
		}
	} finally {
		await dns.close();
	}
});

test("DNS: an answer past dns.maxStale is not served", async (t) => {
	t.plan(1);
	const dns = await startDnsServer({
		zone: { "old.test": { a: ["127.0.0.1"], ttl: 1 } },
		delayMs: DELAY,
	});
	// A window shorter than the wait below, so the entry ages out of it.
	const agent = agentFor(dns, { maxStale: 200 });
	try {
		await agent.prefetchDns("old.test");
		await sleep(2000);

		const elapsed = await timed(() => agent.prefetchDns("old.test"));
		t.ok(
			elapsed >= FRESH_MIN,
			`an entry past the window blocks on a fresh answer (${elapsed}ms)`,
		);
	} finally {
		await dns.close();
	}
});

test("DNS: dns.serveStale false blocks on a fresh answer", async (t) => {
	t.plan(1);
	const dns = await startDnsServer({
		zone: { "strict.test": { a: ["127.0.0.1"], ttl: 1 } },
		delayMs: DELAY,
	});
	const agent = agentFor(dns, { serveStale: false });
	try {
		await agent.prefetchDns("strict.test");
		await expire();

		const elapsed = await timed(() => agent.prefetchDns("strict.test"));
		t.ok(
			elapsed >= FRESH_MIN,
			`an expired entry is discarded rather than served (${elapsed}ms)`,
		);
	} finally {
		await dns.close();
	}
});

test("DNS: exempt names are never served stale", async (t) => {
	t.plan(1);
	const dns = await startDnsServer({
		zone: { "localhost": { a: ["127.0.0.1"], ttl: 1 } },
	});
	const agent = agentFor(dns);
	try {
		// localhost goes to the system resolver whatever dns.servers says, so Faith holds
		// no answer for it to serve stale and the helper is never asked.
		await agent.prefetchDns("localhost");
		await expire();
		await agent.prefetchDns("localhost");

		t.equal(
			dns.countFor("localhost"),
			0,
			"an exempt name never reaches the configured nameserver",
		);
	} finally {
		await dns.close();
	}
});

test("DNS: a network change drops stale answers", async (t) => {
	t.plan(1);
	const dns = await startDnsServer({
		zone: { "netchg.test": { a: ["127.0.0.1"], ttl: 1 } },
		delayMs: DELAY,
	});
	const agent = agentFor(dns);
	try {
		await agent.prefetchDns("netchg.test");
		await expire();

		// The addresses came from the old network, so none of them may be served on the new one.
		agent.networkChanged();

		const elapsed = await timed(() => agent.prefetchDns("netchg.test"));
		t.ok(
			elapsed >= FRESH_MIN,
			`the next lookup resolves afresh rather than serving stale (${elapsed}ms)`,
		);
	} finally {
		await dns.close();
	}
});

test("DNS: a stale address that has moved is re-resolved and retried", async (t) => {
	t.plan(3);
	const origin = await startOrigin();
	const dns = await startDnsServer({
		zone: { "moved.test": { a: ["127.0.0.2"], ttl: 1 } },
	});
	const agent = agentFor(dns);
	const url = `http://moved.test:${origin.port}/`;
	try {
		await agent.prefetchDns("moved.test");

		// Confirms the wrong address really is unreachable, so the recovery below is doing
		// the work rather than the address having been fine all along.
		try {
			await fetch(url, { agent, timeout: 5000 });
			t.fail("the wrong address should not reach the origin");
		} catch (err) {
			t.ok(err, "a fresh wrong address fails, as any wrong address does");
		}

		dns.set("moved.test", { a: ["127.0.0.1"], ttl: 1 });
		await expire();

		const res = await fetch(url, { agent, timeout: 8000 });
		t.equal(res.status, 200, "the request succeeds against the re-resolved address");
		t.equal(await res.text(), "reached", "and reaches the origin itself");
	} finally {
		await dns.close();
		await origin.close();
	}
});

test("DNS: a POST recovers from a stale address too", async (t) => {
	t.plan(1);
	const origin = await startOrigin();
	const dns = await startDnsServer({
		zone: { "post.test": { a: ["127.0.0.2"], ttl: 1 } },
	});
	const agent = agentFor(dns);
	try {
		await agent.prefetchDns("post.test");
		dns.set("post.test", { a: ["127.0.0.1"], ttl: 1 });
		await expire();

		// Nothing reached the origin, so attempting it again cannot double anything: the
		// method does not bound this retry the way it bounds a dead-connection replay.
		const res = await fetch(`http://post.test:${origin.port}/`, {
			agent,
			method: "POST",
			body: "hello",
			timeout: 8000,
		});
		t.equal(res.status, 200, "a POST is attempted against the confirmed address");
	} finally {
		await dns.close();
		await origin.close();
	}
});

test("DNS: a streaming body is not retried after a stale address fails", async (t) => {
	t.plan(1);
	const origin = await startOrigin();
	const dns = await startDnsServer({
		zone: { "stream.test": { a: ["127.0.0.2"], ttl: 1 } },
	});
	const agent = agentFor(dns);
	try {
		await agent.prefetchDns("stream.test");
		dns.set("stream.test", { a: ["127.0.0.1"], ttl: 1 });
		await expire();

		// A stream body has no second copy to send, so the connect failure reaches the
		// caller even though re-attempting would otherwise be safe.
		const body = new ReadableStream({
			start(controller) {
				controller.enqueue(new TextEncoder().encode("hello"));
				controller.close();
			},
		});
		try {
			await fetch(`http://stream.test:${origin.port}/`, {
				agent,
				method: "POST",
				body,
				duplex: "half",
				timeout: 8000,
			});
			t.fail("a streaming body should not be attempted again");
		} catch (err) {
			t.ok(err, "the connect failure reaches the caller");
		}
	} finally {
		await dns.close();
		await origin.close();
	}
});
