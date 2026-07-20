const test = require("tape");
const { Agent, ERROR_CODES } = require("../wrapper.js");

// A standalone self-signed root, used only to exercise PEM parsing of the
// tls.extraRoots option (constructing an Agent does not open any connection).
const SAMPLE_CA = `-----BEGIN CERTIFICATE-----
MIIBiTCCAS+gAwIBAgIUVvXGQmqnPOD4anQtboSJx+wMPXowCgYIKoZIzj0EAwIw
GjEYMBYGA1UEAwwPZmFpdGgtdGVzdC1yb290MB4XDTI2MDcyMDEyNTg0OVoXDTM2
MDcxNzEyNTg0OVowGjEYMBYGA1UEAwwPZmFpdGgtdGVzdC1yb290MFkwEwYHKoZI
zj0CAQYIKoZIzj0DAQcDQgAES0ZXDurE8lWWJ+gUntbc/SKydr0r5n8WFzmb49/L
cVlEUjSPDT1j7MdjACEW+NZofGXOFsk2RWaNgVckLrPDIqNTMFEwHQYDVR0OBBYE
FEy8HeW+3ydWixjkoHNONy3zIZntMB8GA1UdIwQYMBaAFEy8HeW+3ydWixjkoHNO
Ny3zIZntMA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDSAAwRQIhANo4l+K1
pzF7jlVru2Xt8AdtRrizxPgG1UVMn2cPfh+7AiAtr8B9hhDsuXr6KBoQeE4/T2YH
9F3jWAWX24aWTPUITA==
-----END CERTIFICATE-----`;

test("Agent accepts extraRoots as a PEM string", (t) => {
	t.plan(1);
	const agent = new Agent({ tls: { extraRoots: [SAMPLE_CA] } });
	t.ok(agent, "Agent should construct with a PEM-string extra root");
});

test("Agent accepts extraRoots as a Buffer", (t) => {
	t.plan(1);
	const agent = new Agent({ tls: { extraRoots: [Buffer.from(SAMPLE_CA)] } });
	t.ok(agent, "Agent should construct with a Buffer extra root");
});

test("Agent accepts multiple extraRoots", (t) => {
	t.plan(1);
	const agent = new Agent({ tls: { extraRoots: [SAMPLE_CA, SAMPLE_CA] } });
	t.ok(agent, "Agent should construct with multiple extra roots");
});

test("Agent throws on an invalid extraRoots PEM", (t) => {
	t.plan(2);
	try {
		new Agent({ tls: { extraRoots: ["-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----"] } });
		t.fail("should have thrown for an invalid PEM");
	} catch (err) {
		t.ok(err, "should throw for an invalid PEM");
		t.equal(err.code, ERROR_CODES.PemParse, "should set canonical error code 'PemParse'");
	}
});
