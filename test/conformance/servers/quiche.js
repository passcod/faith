/**
 * The quiche row: Cloudflare's QUIC, and a third HTTP/3 implementation.
 *
 * faith's client stack is quinn, so an in-tree quinn server would test one
 * implementation against itself. Caddy brings quic-go. This brings quiche, which is
 * what a large slice of the internet's edge actually answers with.
 *
 * quiche-server is HTTP/3 only -- it has no TCP listener at all -- which shapes two
 * things. It cannot advertise Alt-Svc, so it declares neither ALTSVC nor any TCP
 * protocol, and it cannot be reached by a client that waits to be told about HTTP/3:
 * the row hands the agent a seeded hint through `agentOptions`, which is the whole
 * reason that seam exists.
 *
 * Not on PATH like the others, because there is no package for it: QUICHE_SERVER
 * points at a build, which is what the workflow sets.
 */

const { execFileSync, spawn } = require("node:child_process");
const path = require("node:path");

const { CAPABILITIES: C } = require("../capabilities.js");
const { ensureCert, findFreePort, waitForUdpPort } = require("../../fixtures/net.js");
const { buildStaticTree } = require("./static-tree.js");

/** The binary, from the environment or from PATH. */
function locateBinary() {
	const candidate = process.env.QUICHE_SERVER || "quiche-server";
	try {
		execFileSync(candidate, ["--help"], { stdio: "ignore" });
		return candidate;
	} catch {
		return null;
	}
}

const quiche = {
	name: "quiche",
	expectVersion: "HTTP/3.0",
	// A static file server over QUIC: no compression, no ETags, no trailers, and no
	// TCP protocol to declare. Sparse on purpose -- the row is here for the QUIC
	// stack underneath, and claiming more would produce cells that fail rather than
	// coverage that exists.
	capabilities: new Set([C.H3, C.TLS, C.CONTENT_LENGTH]),

	available() {
		return locateBinary() !== null;
	},

	async start() {
		const binary = locateBinary();
		if (!binary) throw new Error("quiche-server not found; set QUICHE_SERVER to a build");

		const { ca, certPath, keyPath } = ensureCert();
		const port = await findFreePort();
		const tree = buildStaticTree();

		const proc = spawn(
			binary,
			[
				"--listen",
				`127.0.0.1:${port}`,
				"--cert",
				certPath,
				"--key",
				keyPath,
				"--root",
				tree.dir,
			],
			// RUST_LOG so a startup failure has something to say. On success it prints
			// nothing at all, which is why readiness is the bound socket rather than a log
			// line.
			{ stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, RUST_LOG: "info" } },
		);
		let log = "";
		const collect = (chunk) => {
			log += chunk.toString();
		};
		proc.stdout.on("data", collect);
		proc.stderr.on("data", collect);

		const close = async () => {
			proc.kill();
			await new Promise((resolve) => {
				if (proc.exitCode !== null) return resolve();
				proc.once("exit", resolve);
				setTimeout(resolve, 2_000).unref();
			});
			tree.cleanup();
		};

		try {
			await waitForUdpPort({ port, describe: "quiche-server", proc, diagnose: () => log });
		} catch (err) {
			await close();
			throw err;
		}

		return {
			url: `https://localhost:${port}`,
			ca,
			log: () => log,
			// Every cell on this row needs HTTP/3 from its first request, since there is
			// nothing else listening. Without the hint the agent would try TCP, find
			// nothing, and the row would fail as though quiche were broken.
			agentOptions: {
				http3: { upgradeEnabled: true, hints: [{ host: "localhost", port }] },
			},
			close,
		};
	},
};

module.exports = { quiche, locateBinary };
