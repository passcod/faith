const test = require("tape");
const { fetch, Agent, ERROR_CODES } = require("../wrapper.js");
const { url, hasNativeFetch } = require("./helpers.js");

test("Agent accepts a valid localAddress", (t) => {
	t.plan(1);
	const agent = new Agent({ localAddress: "0.0.0.0" });
	t.ok(agent, "Agent should construct with localAddress 0.0.0.0");
});

test("Agent accepts an IPv6 localAddress", (t) => {
	t.plan(1);
	// Constructing an agent doesn't bind anything, so this parses regardless of
	// whether the host has usable IPv6.
	const agent = new Agent({ localAddress: "::" });
	t.ok(agent, "Agent should construct with localAddress ::");
});

test("Agent throws on an invalid localAddress", (t) => {
	t.plan(2);
	try {
		new Agent({ localAddress: "not-an-ip-address" });
		t.fail("should have thrown for an invalid localAddress");
	} catch (err) {
		t.ok(err, "should throw for an invalid localAddress");
		t.equal(
			err.code,
			ERROR_CODES.AddressParse,
			"should set canonical error code 'AddressParse'",
		);
	}
});

test("Agent with localAddress 0.0.0.0 still makes requests", {
	skip: !hasNativeFetch,
}, async (t) => {
	t.plan(1);
	// Binding the IPv4 wildcard must not break ordinary requests. (This is also
	// the address fáith selects automatically on IPv4-only hosts so that the
	// HTTP/3 QUIC socket, which otherwise binds the IPv6 wildcard, can be
	// created instead of silently falling back to TCP.)
	const agent = new Agent({ localAddress: "0.0.0.0" });
	const response = await fetch(url("/get"), { agent });
	t.ok(response.ok, "request bound to 0.0.0.0 should succeed");
});
