/**
 * The controllable DNS server is test infrastructure, so it needs its own test:
 * any verdict about resolver caching depends on it answering what it claims.
 *
 * It's checked against Node's own resolver (c-ares) rather than Faith, so a
 * failure here is the helper's wire format rather than anything about Faith.
 */

const test = require("tape");
const { Resolver } = require("node:dns").promises;
const net = require("node:net");
const { startDnsServer } = require("./lib/dns-server.js");

/** A resolver pointed at the helper, with no patience for a dropped query. */
function resolverFor(server, { timeout = 1000, tries = 1 } = {}) {
	const resolver = new Resolver({ timeout, tries });
	resolver.setServers(server.servers);
	return resolver;
}

/** Encode a bare A query, for reaching the TCP path directly. */
function encodeQuery(id, name, type = 1) {
	const labels = name.split(".").map((label) => {
		const buf = Buffer.alloc(1 + label.length);
		buf.writeUInt8(label.length, 0);
		buf.write(label, 1, "latin1");
		return buf;
	});
	const header = Buffer.alloc(12);
	header.writeUInt16BE(id, 0);
	header.writeUInt16BE(0x0100, 2); // RD
	header.writeUInt16BE(1, 4); // one question
	const tail = Buffer.alloc(5);
	tail.writeUInt8(0, 0); // root label
	tail.writeUInt16BE(type, 1);
	tail.writeUInt16BE(1, 3); // IN
	return Buffer.concat([header, ...labels, tail]);
}

test("dns server: answers A and AAAA from the zone", async (t) => {
	const server = await startDnsServer({
		zone: {
			"dual.test": { a: ["127.0.0.2"], aaaa: ["::1"], ttl: 30 },
		},
	});
	const resolver = resolverFor(server);
	try {
		t.deepEqual(
			await resolver.resolve4("dual.test"),
			["127.0.0.2"],
			"serves the A record",
		);
		t.deepEqual(
			await resolver.resolve6("dual.test"),
			["::1"],
			"serves the AAAA record",
		);
	} finally {
		await server.close();
	}
});

test("dns server: serves several addresses in one answer", async (t) => {
	const server = await startDnsServer({
		zone: { "multi.test": { a: ["127.0.0.2", "127.0.0.3"], ttl: 30 } },
	});
	const resolver = resolverFor(server);
	try {
		const addresses = await resolver.resolve4("multi.test");
		t.deepEqual(
			addresses.slice().sort(),
			["127.0.0.2", "127.0.0.3"],
			"both addresses arrive",
		);
	} finally {
		await server.close();
	}
});

test("dns server: serves the configured TTL", async (t) => {
	const server = await startDnsServer({
		zone: { "ttl.test": { a: ["127.0.0.2"], ttl: 7 } },
	});
	const resolver = resolverFor(server);
	try {
		const [record] = await resolver.resolve4("ttl.test", { ttl: true });
		t.equal(record.ttl, 7, "the TTL is the one the zone set");
	} finally {
		await server.close();
	}
});

test("dns server: a name outside the zone is NXDOMAIN", async (t) => {
	const server = await startDnsServer({
		zone: { "known.test": { a: ["127.0.0.2"] } },
	});
	const resolver = resolverFor(server);
	try {
		await resolver.resolve4("absent.test");
		t.fail("should not resolve a name the zone does not hold");
	} catch (err) {
		t.equal(err.code, "ENOTFOUND", "NXDOMAIN surfaces as ENOTFOUND");
	} finally {
		await server.close();
	}
});

test("dns server: an A-only name answers AAAA with no records", async (t) => {
	// NODATA rather than NXDOMAIN: the name exists, it just holds no AAAA. A
	// resolver querying both families must be able to tell those apart.
	const server = await startDnsServer({
		zone: { "v4only.test": { a: ["127.0.0.2"] } },
	});
	const resolver = resolverFor(server);
	try {
		await resolver.resolve6("v4only.test");
		t.fail("should not invent a AAAA record");
	} catch (err) {
		t.equal(err.code, "ENODATA", "an absent record type is ENODATA");
	} finally {
		await server.close();
	}
});

test("dns server: logs every query it receives", async (t) => {
	const server = await startDnsServer({
		zone: { "logged.test": { a: ["127.0.0.2"], ttl: 30 } },
	});
	const resolver = resolverFor(server);
	try {
		await resolver.resolve4("logged.test");
		await resolver.resolve4("logged.test");
		t.equal(server.countFor("logged.test", "A"), 2, "counts both queries");
		t.equal(server.countFor("logged.test", "AAAA"), 0, "and only those");

		server.resetQueries();
		t.equal(server.countFor("logged.test"), 0, "the log can be reset");

		await resolver.resolve4("logged.test");
		const [entry] = server.queries;
		t.equal(entry.name, "logged.test", "the entry names the query");
		t.equal(entry.type, "A", "and its type");
		t.equal(entry.transport, "udp", "and how it arrived");
	} finally {
		await server.close();
	}
});

test("dns server: set() changes the answer mid-run", async (t) => {
	const server = await startDnsServer({
		zone: { "moving.test": { a: ["127.0.0.2"], ttl: 30 } },
	});
	const resolver = resolverFor(server);
	try {
		t.deepEqual(
			await resolver.resolve4("moving.test"),
			["127.0.0.2"],
			"the original address",
		);

		server.set("moving.test", { a: ["127.0.0.3"], ttl: 30 });
		t.deepEqual(
			await resolver.resolve4("moving.test"),
			["127.0.0.3"],
			"the replacement address, with no restart",
		);

		server.set("moving.test", null);
		try {
			await resolver.resolve4("moving.test");
			t.fail("a removed name should stop resolving");
		} catch (err) {
			t.equal(err.code, "ENOTFOUND", "a removed name is NXDOMAIN");
		}
	} finally {
		await server.close();
	}
});

test("dns server: set() rejects a malformed address", async (t) => {
	const server = await startDnsServer();
	try {
		t.throws(
			() => server.set("bad.test", { a: ["not-an-address"] }),
			/not an IPv4 address/,
			"a bad address is the caller's error, not a later SERVFAIL",
		);
	} finally {
		await server.close();
	}
});

test("dns server: fail() forces SERVFAIL and NXDOMAIN", async (t) => {
	const server = await startDnsServer({
		zone: { "failing.test": { a: ["127.0.0.2"], ttl: 30 } },
	});
	const resolver = resolverFor(server);
	try {
		server.fail("failing.test", "servfail");
		try {
			await resolver.resolve4("failing.test");
			t.fail("SERVFAIL should not resolve");
		} catch (err) {
			t.equal(err.code, "ESERVFAIL", "SERVFAIL surfaces as such");
		}

		server.fail("failing.test", "nxdomain");
		try {
			await resolver.resolve4("failing.test");
			t.fail("NXDOMAIN should not resolve");
		} catch (err) {
			t.equal(err.code, "ENOTFOUND", "a forced NXDOMAIN surfaces too");
		}

		server.fail("failing.test", null);
		t.deepEqual(
			await resolver.resolve4("failing.test"),
			["127.0.0.2"],
			"clearing the failure restores the zone answer",
		);
	} finally {
		await server.close();
	}
});

test("dns server: fail('drop') never answers", async (t) => {
	const server = await startDnsServer({
		zone: { "silent.test": { a: ["127.0.0.2"], ttl: 30 } },
	});
	const resolver = resolverFor(server, { timeout: 300 });
	try {
		server.fail("silent.test", "drop");
		try {
			await resolver.resolve4("silent.test");
			t.fail("a dropped query should not resolve");
		} catch (err) {
			t.equal(err.code, "ETIMEOUT", "the client times out");
		}
		t.ok(
			server.countFor("silent.test") >= 1,
			"the query still reached the server and was logged",
		);
	} finally {
		await server.close();
	}
});

test("dns server: setDelay() slows the answer", async (t) => {
	const server = await startDnsServer({
		zone: { "slow.test": { a: ["127.0.0.2"], ttl: 30 } },
	});
	const resolver = resolverFor(server, { timeout: 5000 });
	try {
		server.setDelay(300);
		const started = Date.now();
		t.deepEqual(
			await resolver.resolve4("slow.test"),
			["127.0.0.2"],
			"a delayed answer still arrives intact",
		);
		const elapsed = Date.now() - started;
		t.ok(elapsed >= 250, `the answer waited for the delay (${elapsed}ms)`);

		server.setDelay(0);
		const fastStart = Date.now();
		await resolver.resolve4("slow.test");
		t.ok(
			Date.now() - fastStart < 250,
			"and the delay can be turned back off",
		);
	} finally {
		await server.close();
	}
});

test("dns server: answers over TCP as well as UDP", async (t) => {
	// Hickory falls back to TCP when a UDP exchange fails, so a UDP-only helper
	// would turn that fallback into a hang.
	const server = await startDnsServer({
		zone: { "tcp.test": { a: ["127.0.0.2"], ttl: 30 } },
	});
	try {
		const response = await new Promise((resolve, reject) => {
			const socket = net.connect(server.port, server.host, () => {
				const message = encodeQuery(0x1234, "tcp.test");
				const framed = Buffer.alloc(2 + message.length);
				framed.writeUInt16BE(message.length, 0);
				message.copy(framed, 2);
				socket.write(framed);
			});
			let buffered = Buffer.alloc(0);
			socket.on("data", (chunk) => {
				buffered = Buffer.concat([buffered, chunk]);
				if (buffered.length < 2) return;
				const length = buffered.readUInt16BE(0);
				if (buffered.length < 2 + length) return;
				socket.destroy();
				resolve(buffered.subarray(2, 2 + length));
			});
			socket.on("error", reject);
			socket.setTimeout(2000, () => {
				socket.destroy();
				reject(new Error("timed out waiting for a TCP answer"));
			});
		});

		t.equal(response.readUInt16BE(0), 0x1234, "the reply carries the query id");
		t.equal(response.readUInt16BE(2) & 0x8000, 0x8000, "and is a response");
		t.equal(response.readUInt16BE(2) & 0x000f, 0, "with rcode NOERROR");
		t.equal(response.readUInt16BE(6), 1, "and one answer record");
		// Answer starts after the 12-byte header and the echoed question.
		const answer = response.subarray(12 + "tcp.test".length + 2 + 4);
		t.equal(answer.readUInt16BE(0), 0xc00c, "whose name points at the question");
		t.equal(answer.readUInt32BE(6), 30, "carrying the zone's TTL");
		t.deepEqual(
			Array.from(answer.subarray(12, 16)),
			[127, 0, 0, 2],
			"and the configured address",
		);
		t.equal(
			server.queries.filter((q) => q.transport === "tcp").length,
			1,
			"the query is logged as TCP",
		);
	} finally {
		await server.close();
	}
});

test("dns server: matches names case-insensitively", async (t) => {
	// Hickory can randomise the case of a query name as a spoofing defence, and
	// drops a reply whose echoed question doesn't match byte for byte.
	const server = await startDnsServer({
		zone: { "MiXeD.test": { a: ["127.0.0.2"], ttl: 30 } },
	});
	try {
		const response = await new Promise((resolve, reject) => {
			const socket = net.connect(server.port, server.host, () => {
				const message = encodeQuery(0x4321, "mIxEd.TEST");
				const framed = Buffer.alloc(2 + message.length);
				framed.writeUInt16BE(message.length, 0);
				message.copy(framed, 2);
				socket.write(framed);
			});
			let buffered = Buffer.alloc(0);
			socket.on("data", (chunk) => {
				buffered = Buffer.concat([buffered, chunk]);
				if (buffered.length < 2) return;
				const length = buffered.readUInt16BE(0);
				if (buffered.length < 2 + length) return;
				socket.destroy();
				resolve(buffered.subarray(2, 2 + length));
			});
			socket.on("error", reject);
			socket.setTimeout(2000, () => {
				socket.destroy();
				reject(new Error("timed out waiting for a TCP answer"));
			});
		});

		t.equal(response.readUInt16BE(6), 1, "the mixed-case name still matches");
		t.equal(
			response.subarray(13, 13 + "mIxEd".length).toString("latin1"),
			"mIxEd",
			"and the question is echoed with its case preserved",
		);
	} finally {
		await server.close();
	}
});
