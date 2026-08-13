const test = require("tape");
const { fetch: faithFetch, Agent, ERROR_CODES } = require("../wrapper.js");
const { url } = require("./helpers.js");

// A name that is neither exempt nor overridden, so it reaches the configured resolver, and a
// server address that refuses fast, so the lookup fails without a public DNS call. The lookup
// failing is fine: the resolver is still built, which is what `resolvers()` reports.
const NON_EXEMPT = "http://nonexistent-faith-test.example/";
const DEAD = "udp://127.0.0.1:1";

test("dns.servers combined with dns.system throws at construction", (t) => {
	t.plan(2);
	try {
		new Agent({ dns: { system: true, servers: ["udp://1.1.1.1"] } });
		t.fail("should have thrown");
	} catch (err) {
		t.ok(err, "should throw");
		t.equal(err.code, ERROR_CODES.Config, "carries the Config code");
	}
});

test("an unparseable dns.servers URL throws AddressParse at construction", (t) => {
	t.plan(2);
	try {
		new Agent({ dns: { servers: ["not a url"] } });
		t.fail("should have thrown");
	} catch (err) {
		t.ok(err, "should throw");
		t.equal(err.code, ERROR_CODES.AddressParse, "carries the AddressParse code");
	}
});

test("an unknown dns.servers scheme throws AddressParse at construction", (t) => {
	t.plan(1);
	try {
		new Agent({ dns: { servers: ["ftp://1.1.1.1"] } });
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.AddressParse, "carries the AddressParse code");
	}
});

test("dns.servers accepts every transport scheme", (t) => {
	t.plan(1);
	const agent = new Agent({
		dns: {
			servers: [
				"udp://1.1.1.1",
				"tcp://1.1.1.1",
				"tls://1.1.1.1#cloudflare-dns.com",
				"https://dns.google",
				"quic://1.1.1.1",
				"h3://1.1.1.1",
			],
		},
	});
	t.ok(agent, "constructs with all transports");
});

test("resolvers() is empty before the resolver is used", (t) => {
	t.plan(1);
	const agent = new Agent({ dns: { servers: [DEAD], timeout: 500 } });
	t.deepEqual(agent.resolvers(), [], "lists nothing until first use");
});

test("resolvers() is empty for the system resolver", (t) => {
	t.plan(1);
	const agent = new Agent({ dns: { system: true } });
	t.deepEqual(agent.resolvers(), [], "the system resolver reports nothing");
});

test("resolvers() reports configured servers in order with their transports", async (t) => {
	t.plan(7);
	const agent = new Agent({
		dns: {
			servers: ["tls://127.0.0.1:1#test", DEAD],
			timeout: 500,
		},
	});

	try {
		await faithFetch(NON_EXEMPT, { agent });
	} catch {
		// The dead servers can't answer; we only care that the resolver was built.
	}

	const resolvers = agent.resolvers();
	t.equal(resolvers.length, 2, "reports both configured servers");
	t.equal(resolvers[0].transport, "tls", "first server keeps its transport");
	t.equal(resolvers[0].address, "127.0.0.1:1", "first server keeps its address");
	t.equal(resolvers[0].source, "configured", "first server is configured");
	t.equal(resolvers[1].transport, "udp", "second server keeps its transport");
	t.equal(resolvers[1].address, "127.0.0.1:1", "second server keeps its address");
	t.equal(resolvers[1].source, "configured", "second server is configured");
});

test("an explicit port and default query path shape the reported address", async (t) => {
	t.plan(2);
	const agent = new Agent({
		dns: { servers: ["https://127.0.0.1:8443"], timeout: 500 },
	});

	try {
		await faithFetch(NON_EXEMPT, { agent });
	} catch {}

	const resolvers = agent.resolvers();
	t.equal(resolvers[0].transport, "https", "https transport");
	t.equal(resolvers[0].address, "127.0.0.1:8443", "explicit port is kept");
});

test("localhost is exempt from configured servers, so requests still resolve", async (t) => {
	t.plan(2);
	// The only configured server is dead, but localhost is exempt and goes to the system
	// resolver, so a request to the local httpbin still succeeds (spec:DNS#exempt-names).
	const agent = new Agent({
		dns: { servers: [DEAD], timeout: 1000 },
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "localhost resolves via the system resolver");
	t.equal(response.status, 200, "status is 200");
});

test("networkChanged returns resolvers() to unbuilt, then it rebuilds as configured", async (t) => {
	t.plan(4);
	// The listed servers are read off the network on first use, so the signal drops them and the
	// next lookup reads again; the caller's list is configuration and survives (spec:DNS#network-changes).
	const agent = new Agent({
		dns: { servers: ["tls://127.0.0.1:1#test", DEAD], timeout: 500 },
	});

	try {
		await faithFetch(NON_EXEMPT, { agent });
	} catch {}
	t.equal(agent.resolvers().length, 2, "the resolver is built and reported");

	agent.networkChanged();
	t.deepEqual(agent.resolvers(), [], "the signal returns it to reporting nothing");

	try {
		await faithFetch(NON_EXEMPT, { agent });
	} catch {}
	const rebuilt = agent.resolvers();
	t.equal(rebuilt.length, 2, "the next lookup rebuilds the list");
	t.equal(rebuilt[0].transport, "tls", "as configured, in the configured order");
});

test("networkChanged leaves exempt names resolving through the system resolver", async (t) => {
	t.plan(2);
	// The exempt suffixes are re-read with everything else, so localhost stays exempt and keeps
	// resolving after the signal (spec:DNS#network-changes).
	const agent = new Agent({ dns: { servers: [DEAD], timeout: 1000 } });

	const before = await faithFetch(url("/get"), { agent });
	t.ok(before.ok, "resolves before the signal");
	await before.text();

	agent.networkChanged();

	const after = await faithFetch(url("/get"), { agent });
	t.ok(after.ok, "and after it");
	await after.text();
});

test("dns name-preparation and exemption options construct cleanly", async (t) => {
	t.plan(1);
	const agent = new Agent({
		dns: {
			servers: [DEAD],
			searchDomains: ["internal.example", "corp.example"],
			ndots: 2,
			hostsFile: true,
			exemptDomains: ["internal.example"],
			timeout: 1000,
		},
	});

	const response = await faithFetch(url("/get"), { agent });
	t.ok(response.ok, "constructs and resolves localhost with all name options set");
});
