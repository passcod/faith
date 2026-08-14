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

const dgram = require("node:dgram");
const net = require("node:net");

const { ensureCert, findFreePort } = require("./net.js");
const { startCaddy, caddyAvailable } = require("./caddy.js");

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
			// Every socket gets closed even if one of them objects: a dgram socket that
			// is already down throws on close, and one throw part-way would strand the
			// rest, where a bound UDP socket holds the event loop and hangs the whole run
			// long after the last assertion.
			for (const socket of [sock, ...upstreams.values()]) {
				try {
					socket.close();
				} catch {
					// already down, which is what was wanted
				}
			}
			upstreams.clear();
		},
	};
}

module.exports = {
	ensureCert,
	findFreePort,
	startCaddy,
	startTcpProxy,
	startUdpRelay,
	caddyAvailable,
};
