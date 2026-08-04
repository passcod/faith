/**
 * Harness for testing HTTP/3 behaviour when the UDP path breaks.
 *
 * Topology (single loopback IP, so it works the same on every platform):
 *
 *   client ──> https://localhost:FRONT
 *        TCP  127.0.0.1:FRONT ──[tcp proxy]──> 127.0.0.1:BACK   always healthy
 *        UDP  127.0.0.1:FRONT ──[udp relay]──> 127.0.0.1:BACK   blackhole switch
 *
 *   Caddy serves both TCP (h1/h2) and UDP (h3) on BACK.
 *
 * The relay is the fault injector: when blackholed it silently discards
 * datagrams in both directions. That is deliberately not `iptables -j DROP`:
 * dropping in userspace needs no privileges, and it produces no ICMP at all,
 * which is the "hangs, with no error ever surfacing" condition we need to
 * reproduce. A REJECT rule would produce an error and hide the bug.
 */

const { execFileSync, spawn } = require("node:child_process");
const { existsSync, readFileSync, writeFileSync, mkdirSync } = require("node:fs");
const dgram = require("node:dgram");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");

/**
 * Private CA plus a leaf for localhost, cached in tmp. A plain self-signed cert
 * isn't enough: rustls refuses to accept a CA certificate as an end-entity, so
 * the server presents the leaf and the client trusts the CA.
 */
function ensureCert() {
	const dir = path.join(os.tmpdir(), "faith-test-cert-v1");
	const caKeyPath = path.join(dir, "ca-key.pem");
	const caPath = path.join(dir, "ca.pem");
	const keyPath = path.join(dir, "key.pem");
	const csrPath = path.join(dir, "leaf.csr");
	const certPath = path.join(dir, "cert.pem");
	if (!existsSync(caPath) || !existsSync(keyPath) || !existsSync(certPath)) {
		mkdirSync(dir, { recursive: true });
		const ec = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1"];
		execFileSync("openssl", [
			"req", "-x509", ...ec, "-keyout", caKeyPath, "-out", caPath,
			"-days", "30", "-nodes", "-subj", "/CN=faith-test-ca",
			"-addext", "basicConstraints=critical,CA:TRUE",
		]);
		execFileSync("openssl", [
			"req", "-new", ...ec, "-keyout", keyPath, "-out", csrPath,
			"-nodes", "-subj", "/CN=localhost",
			"-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1",
			"-addext", "basicConstraints=critical,CA:FALSE",
		]);
		execFileSync("openssl", [
			"x509", "-req", "-in", csrPath, "-CA", caPath, "-CAkey", caKeyPath,
			"-CAcreateserial", "-out", certPath, "-days", "30",
			"-copy_extensions", "copyall",
		]);
	}
	return { ca: readFileSync(caPath), caPath, certPath, keyPath };
}

/** A port free for both TCP and UDP on 127.0.0.1. */
async function findFreePort() {
	for (let attempt = 0; attempt < 20; attempt++) {
		const port = await new Promise((resolve, reject) => {
			const srv = net.createServer();
			srv.once("error", reject);
			srv.listen(0, "127.0.0.1", () => {
				const { port } = srv.address();
				srv.close(() => resolve(port));
			});
		});
		const udpFree = await new Promise((resolve) => {
			const sock = dgram.createSocket("udp4");
			sock.once("error", () => resolve(false));
			sock.bind(port, "127.0.0.1", () => sock.close(() => resolve(true)));
		});
		if (udpFree) return port;
	}
	throw new Error("could not find a port free for both TCP and UDP");
}

/**
 * Spawn Caddy serving https://localhost:<port> over h1/h2/h3.
 *
 * `altSvc` overrides the Alt-Svc header Caddy would emit for itself, so a test can
 * advertise an endpoint that isn't Caddy — a port with nothing listening, say.
 */
async function startCaddy({ port, dir, altSvc }) {
	const { certPath, keyPath } = ensureCert();
	const caddyfile = `{
	auto_https off
	admin off
	servers {
		protocols h1 h2 h3
	}
}

https://localhost:${port} {
	tls ${certPath} ${keyPath}
${altSvc ? `\theader Alt-Svc \`${altSvc}\`\n` : ""}	respond "hello-from-caddy"
}
`;
	const configPath = path.join(dir, `Caddyfile.${port}`);
	writeFileSync(configPath, caddyfile);

	const proc = spawn("caddy", ["run", "--config", configPath, "--adapter", "caddyfile"], {
		stdio: ["ignore", "pipe", "pipe"],
	});
	let log = "";
	proc.stdout.on("data", (c) => { log += c.toString(); });
	proc.stderr.on("data", (c) => { log += c.toString(); });

	await new Promise((resolve, reject) => {
		const deadline = Date.now() + 20000;
		const tick = setInterval(() => {
			if (/serving initial configuration/.test(log)) {
				clearInterval(tick);
				resolve();
			} else if (proc.exitCode !== null || Date.now() > deadline) {
				clearInterval(tick);
				// Kill before rejecting: the caller never gets a handle to close, and a
				// surviving child with piped stdio keeps node alive, so a config error
				// would surface as a hung test run rather than a failure.
				proc.kill();
				reject(new Error(`caddy failed to start:\n${log}`));
			}
		}, 100);
	});

	return { port, log: () => log, close: () => proc.kill() };
}

/** TCP passthrough, always healthy — this is the path the client should fall back to. */
async function startTcpProxy({ listenPort, upstreamPort }) {
	const live = new Set();
	const server = net.createServer((client) => {
		const up = net.connect(upstreamPort, "127.0.0.1");
		live.add(client);
		live.add(up);
		client.pipe(up);
		up.pipe(client);
		const bye = () => {
			live.delete(client);
			live.delete(up);
			client.destroy();
			up.destroy();
		};
		client.on("error", bye);
		up.on("error", bye);
		client.on("close", bye);
		up.on("close", bye);
	});
	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(listenPort, "127.0.0.1", resolve);
	});
	return {
		close: () =>
			new Promise((resolve) => {
				// Tear the sockets down rather than waiting on them. The agent under test
				// keeps its connection pooled, so a bare server.close() never settles and
				// the calling test hangs in its `finally` instead of finishing.
				for (const socket of live) socket.destroy();
				live.clear();
				server.close(resolve);
				setTimeout(resolve, 500).unref();
			}),
	};
}

/** UDP relay with a blackhole switch. */
async function startUdpRelay({ listenPort, upstreamPort }) {
	const sock = dgram.createSocket("udp4");
	const upstreams = new Map();
	const state = { blackhole: false, dropped: 0, forwarded: 0 };

	sock.on("message", (msg, rinfo) => {
		if (state.blackhole) return void state.dropped++;
		const key = `${rinfo.address}:${rinfo.port}`;
		let up = upstreams.get(key);
		if (!up) {
			up = dgram.createSocket("udp4");
			up.on("message", (reply) => {
				if (state.blackhole) return void state.dropped++;
				state.forwarded++;
				sock.send(reply, rinfo.port, rinfo.address);
			});
			up.bind();
			upstreams.set(key, up);
		}
		state.forwarded++;
		up.send(msg, upstreamPort, "127.0.0.1");
	});

	await new Promise((resolve, reject) => {
		sock.once("error", reject);
		sock.bind(listenPort, "127.0.0.1", resolve);
	});

	return {
		state,
		blackhole() { state.blackhole = true; },
		restore() { state.blackhole = false; },
		close() {
			sock.close();
			for (const up of upstreams.values()) up.close();
		},
	};
}

/** True if a `caddy` binary is on PATH. */
function caddyAvailable() {
	try {
		execFileSync("caddy", ["version"], { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

module.exports = {
	ensureCert,
	findFreePort,
	startCaddy,
	startTcpProxy,
	startUdpRelay,
	caddyAvailable,
};
