/**
 * Network fixtures shared by the HTTP/3 tests and the conformance harness.
 *
 * Extracted from h3-blackhole.js so conformance servers can reuse the
 * certificate and port helpers without pulling in the UDP fault-injection
 * machinery, which is specific to the HTTP/3 fallback tests.
 */

const { execFileSync } = require("node:child_process");
const { existsSync, readFileSync, mkdirSync } = require("node:fs");
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
 * Resolve once something accepts TCP on `port`.
 *
 * The configured servers -- nginx, Apache, HAProxy -- print nothing predictable
 * to stdout when they come up, and what they do print goes to a log file inside
 * their own prefix, so the listener accepting is the readiness signal. `diagnose`
 * reads that log, and a server that died during startup fails immediately with its
 * own explanation rather than after the full timeout with none.
 */
async function waitForPort({ port, host = "127.0.0.1", timeoutMs = 15_000, describe, proc, diagnose }) {
	const deadline = Date.now() + timeoutMs;
	for (;;) {
		if (proc && proc.exitCode !== null) {
			throw new Error(
				`${describe} exited with ${proc.exitCode} during startup${detail(diagnose)}`,
			);
		}
		const open = await new Promise((resolve) => {
			const socket = net.connect(port, host);
			const done = (ok) => {
				socket.destroy();
				resolve(ok);
			};
			socket.once("connect", () => done(true));
			socket.once("error", () => done(false));
			socket.setTimeout(500, () => done(false));
		});
		if (open) return;
		if (Date.now() > deadline) {
			throw new Error(
				`${describe} did not accept connections on ${host}:${port} within ` +
					`${timeoutMs}ms${detail(diagnose)}`,
			);
		}
		await new Promise((resolve) => setTimeout(resolve, 50));
	}
}

function detail(diagnose) {
	if (!diagnose) return "";
	try {
		const log = diagnose();
		return log ? `:\n${log}` : "";
	} catch {
		// The log is a nicety; failing to read it must not replace the real error.
		return "";
	}
}

module.exports = { ensureCert, findFreePort, waitForPort };
