// Revocation checking's option plumbing and the shape of its stats. Refusals themselves are
// tested on the Rust side, against a fake Remote Settings server (crates/web-faith/tests).
const test = require("tape");
const { Agent } = require("../wrapper.js");

function isSourceStats(t, stats, name) {
	t.ok(stats, `${name} is reported`);
	t.equal(stats.refused, 0, `${name} has refused nothing yet`);
	t.ok(
		stats.lastConfirmed === null || stats.lastConfirmed instanceof Date,
		`${name}.lastConfirmed is a Date or null`,
	);
	t.ok(
		["remote-settings", "upki", null].includes(stats.provider),
		`${name}.provider is a known provider or null`,
	);
	t.equal(
		stats.provider === null,
		stats.lastConfirmed === null,
		`${name}.provider is null only without data`,
	);
}

test("both revocation sources are on by default", (t) => {
	const stats = new Agent().stats();
	isSourceStats(t, stats.onecrl, "onecrl");
	isSourceStats(t, stats.crlite, "crlite");
	t.end();
});

test("OneCRL always comes from Remote Settings", (t) => {
	const { onecrl } = new Agent().stats();
	t.ok(onecrl.provider === null || onecrl.provider === "remote-settings", "onecrl provider");
	t.end();
});

test("upki can be turned off for an agent", (t) => {
	const off = new Agent({ tls: { revocation: { upki: false } } }).stats();
	isSourceStats(t, off.crlite, "crlite");
	t.notEqual(off.crlite.provider, "upki", "CRLite is not from upki");

	const crliteOff = new Agent({ tls: { revocation: { crlite: false, upki: true } } }).stats();
	t.equal(crliteOff.crlite, null, "upki has no effect with CRLite off");
	t.end();
});

test("a source turned off is not reported", (t) => {
	const onecrlOff = new Agent({ tls: { revocation: { onecrl: false } } }).stats();
	t.equal(onecrlOff.onecrl, null, "onecrl is off");
	isSourceStats(t, onecrlOff.crlite, "crlite");

	const crliteOff = new Agent({ tls: { revocation: { crlite: false } } }).stats();
	isSourceStats(t, crliteOff.onecrl, "onecrl");
	t.equal(crliteOff.crlite, null, "crlite is off");

	const bothOff = new Agent({
		tls: { revocation: { onecrl: false, crlite: false } },
	}).stats();
	t.equal(bothOff.onecrl, null, "onecrl is off");
	t.equal(bothOff.crlite, null, "crlite is off");
	t.end();
});

test("an empty revocation group keeps the defaults", (t) => {
	const stats = new Agent({ tls: { revocation: {} } }).stats();
	isSourceStats(t, stats.onecrl, "onecrl");
	isSourceStats(t, stats.crlite, "crlite");
	t.end();
});

test("NODE_TLS_REJECT_UNAUTHORIZED=0 turns revocation off too", (t) => {
	const saved = process.env.NODE_TLS_REJECT_UNAUTHORIZED;
	process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";
	try {
		const stats = new Agent().stats();
		t.equal(stats.onecrl, null, "onecrl is off");
		t.equal(stats.crlite, null, "crlite is off");
	} finally {
		if (saved === undefined) delete process.env.NODE_TLS_REJECT_UNAUTHORIZED;
		else process.env.NODE_TLS_REJECT_UNAUTHORIZED = saved;
	}
	t.end();
});
