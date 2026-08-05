const test = require("tape");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const http = require("node:http");
const { Agent, fetch } = require("../wrapper.js");

// A standalone self-signed root, used only to exercise PEM parsing of the
// NODE_EXTRA_CA_CERTS file (constructing an Agent does not open any connection).
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

function withEnv(vars, fn) {
	const saved = {};
	for (const [k, v] of Object.entries(vars)) {
		saved[k] = process.env[k];
		if (v === undefined) delete process.env[k];
		else process.env[k] = v;
	}
	const restore = () => {
		for (const [k, v] of Object.entries(saved)) {
			if (v === undefined) delete process.env[k];
			else process.env[k] = v;
		}
	};
	let result;
	try {
		result = fn();
	} catch (err) {
		restore();
		throw err;
	}
	if (result && typeof result.then === "function") {
		return result.finally(restore);
	}
	restore();
	return result;
}

test("Agent loads NODE_EXTRA_CA_CERTS from a PEM file", (t) => {
	t.plan(1);
	const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "faith-ca-")), "ca.pem");
	fs.writeFileSync(file, SAMPLE_CA);
	withEnv({ NODE_EXTRA_CA_CERTS: file }, () => {
		const agent = new Agent();
		t.ok(agent, "Agent should construct with NODE_EXTRA_CA_CERTS set");
	});
	fs.rmSync(path.dirname(file), { recursive: true, force: true });
});

test("Agent ignores a missing NODE_EXTRA_CA_CERTS file", (t) => {
	t.plan(1);
	withEnv({ NODE_EXTRA_CA_CERTS: "/nonexistent/faith/ca.pem" }, () => {
		const agent = new Agent();
		t.ok(agent, "Agent should construct, ignoring a missing CA file");
	});
});

test("Agent ignores an unparseable NODE_EXTRA_CA_CERTS file", (t) => {
	// Unlike the explicit tls.extraRoots option (which throws on bad PEM), the
	// ambient env var is lenient and matches Node.js's warn-and-continue.
	t.plan(1);
	const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "faith-ca-")), "bad.pem");
	fs.writeFileSync(file, "-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----");
	withEnv({ NODE_EXTRA_CA_CERTS: file }, () => {
		const agent = new Agent();
		t.ok(agent, "Agent should construct, ignoring an unparseable CA file");
	});
	fs.rmSync(path.dirname(file), { recursive: true, force: true });
});

test("Agent ignores an empty NODE_EXTRA_CA_CERTS value", (t) => {
	t.plan(1);
	withEnv({ NODE_EXTRA_CA_CERTS: "" }, () => {
		const agent = new Agent();
		t.ok(agent, "Agent should construct with an empty NODE_EXTRA_CA_CERTS");
	});
});

test("Agent constructs with NODE_TLS_REJECT_UNAUTHORIZED=0", (t) => {
	t.plan(1);
	withEnv({ NODE_TLS_REJECT_UNAUTHORIZED: "0" }, () => {
		const agent = new Agent();
		t.ok(agent, "Agent should construct with certificate validation disabled");
	});
});

test("Agent constructs with NODE_TLS_REJECT_UNAUTHORIZED left enabled", (t) => {
	t.plan(1);
	withEnv({ NODE_TLS_REJECT_UNAUTHORIZED: "1" }, () => {
		const agent = new Agent();
		t.ok(agent, "Agent should construct with validation enabled");
	});
});

// Stand up a direct target and a tagging forward-proxy on loopback, point the
// agent's ambient proxy config at the proxy, and observe which one answers.
// NO_PROXY is cleared so loopback isn't bypassed, and both are restored after.
function withProxyServers(fn) {
	return new Promise((resolve, reject) => {
		const target = http.createServer((req, res) => res.end("DIRECT"));
		const proxy = http.createServer((req, res) => {
			res.writeHead(200);
			res.end("VIA-PROXY");
		});
		target.listen(0, "127.0.0.1", () => {
			proxy.listen(0, "127.0.0.1", async () => {
				const url = `http://127.0.0.1:${target.address().port}/`;
				const proxyUrl = `http://127.0.0.1:${proxy.address().port}`;
				try {
					resolve(await fn(url, proxyUrl));
				} catch (err) {
					reject(err);
				} finally {
					target.close();
					proxy.close();
				}
			});
		});
	});
}

async function routeVia(url, proxyUrl, nodeUseEnvProxy) {
	// Only the canonical uppercase spellings are set: reqwest reads proxy env
	// case-insensitively, and on Windows process.env itself is case-insensitive,
	// so saving/restoring both spellings would double-handle the same variable
	// and leak it into later tests.
	return withEnv(
		{
			HTTP_PROXY: proxyUrl,
			NO_PROXY: "",
			NODE_USE_ENV_PROXY: nodeUseEnvProxy,
		},
		async () => {
			const agent = new Agent();
			const response = await fetch(url, { agent });
			return response.text();
		},
	);
}

test("agent uses the ambient proxy by default", async (t) => {
	t.plan(1);
	await withProxyServers(async (url, proxyUrl) => {
		t.equal(await routeVia(url, proxyUrl, undefined), "VIA-PROXY", "unset should proxy");
	});
});

test("NODE_USE_ENV_PROXY=1 keeps the ambient proxy", async (t) => {
	t.plan(1);
	await withProxyServers(async (url, proxyUrl) => {
		t.equal(await routeVia(url, proxyUrl, "1"), "VIA-PROXY", "=1 should proxy");
	});
});

test("NODE_USE_ENV_PROXY=0 bypasses the ambient proxy", async (t) => {
	t.plan(1);
	await withProxyServers(async (url, proxyUrl) => {
		t.equal(await routeVia(url, proxyUrl, "0"), "DIRECT", "=0 should go direct");
	});
});
