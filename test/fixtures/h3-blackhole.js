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
const { writeFileSync } = require("node:fs");
const dgram = require("node:dgram");
const net = require("node:net");
const path = require("node:path");

const { ensureCert, findFreePort } = require("./net.js");

/**
 * Spawn Caddy serving https://localhost:<port> over h1/h2/h3.
 *
 * `altSvc` overrides the Alt-Svc header Caddy would emit for itself, so a test can
 * advertise an endpoint that isn't Caddy — a port with nothing listening, say.
 *
 * `cacheControl` adds a Cache-Control header, so responses become storable by an
 * agent configured with a cache store.
 */
async function startCaddy({ port, dir, altSvc, cacheControl }) {
	const { certPath, keyPath } = ensureCert();
	// Backtick-quoted, because these values contain the double quotes Caddy would
	// otherwise treat as the end of the token.
	const directives = [`tls ${certPath} ${keyPath}`];
	if (altSvc) directives.push(`header Alt-Svc \`${altSvc}\``);
	if (cacheControl) directives.push(`header Cache-Control \`${cacheControl}\``);
	directives.push(`respond "hello-from-caddy"`);

	const caddyfile = `{
	auto_https off
	admin off
	servers {
		protocols h1 h2 h3
	}
}

https://localhost:${port} {
${directives.map((d) => `\t${d}`).join("\n")}
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
