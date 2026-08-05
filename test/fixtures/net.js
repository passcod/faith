/**
 * Network fixtures shared by the HTTP/3 tests and the conformance harness.
 *
 * Extracted from h3-blackhole.js so conformance servers can reuse the
 * certificate and port helpers without pulling in the UDP fault-injection
 * machinery, which is specific to the HTTP/3 fallback tests.
 */

const { execFileSync } = require("node:child_process");
const { existsSync, readFileSync, mkdirSync, writeFileSync } = require("node:fs");
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
 * The leaf certificate and its key in one file.
 *
 * HAProxy's `crt` takes a single PEM containing both, unlike every other server
 * here, which takes two paths. Written next to the pair it is built from and cached
 * the same way.
 */
function ensureCombinedCert() {
	const { ca, caPath, certPath, keyPath } = ensureCert();
	const combinedPath = path.join(path.dirname(certPath), "combined.pem");
	if (!existsSync(combinedPath)) {
		writeFileSync(combinedPath, `${readFileSync(keyPath)}${readFileSync(certPath)}`);
	}
	return { ca, caPath, certPath, keyPath, combinedPath };
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
		if (open) {
			// An open port is not proof that *this* process opened it. A server that
			// failed to bind -- because something else already had the port -- would
			// otherwise be reported ready, and every request would go to whatever is
			// actually listening. That happened: a proxy row talked to its own backend
			// for a while before anyone noticed. So settle, then confirm the process is
			// still alive.
			await new Promise((resolve) => setTimeout(resolve, 100));
			if (proc && proc.exitCode !== null) {
				throw new Error(
					`${describe} exited with ${proc.exitCode} while ${host}:${port} was open, ` +
						`so something else is listening there${detail(diagnose)}`,
				);
			}
			return;
		}
		if (Date.now() > deadline) {
			throw new Error(
				`${describe} did not accept connections on ${host}:${port} within ` +
					`${timeoutMs}ms${detail(diagnose)}`,
			);
		}
		await new Promise((resolve) => setTimeout(resolve, 50));
	}
}

/**
 * Resolve once something holds the UDP port.
 *
 * There is no UDP equivalent of a connect, and an HTTP/3-only server has no TCP
 * listener to probe: quiche-server prints nothing on startup even at RUST_LOG=info,
 * so its log is no help either. What is observable is the socket itself -- once the
 * server has bound the port, binding it here fails with EADDRINUSE, and that
 * failure is the readiness signal.
 */
async function waitForUdpPort({ port, host = "127.0.0.1", timeoutMs = 15_000, describe, proc, diagnose }) {
	const deadline = Date.now() + timeoutMs;
	for (;;) {
		if (proc && proc.exitCode !== null) {
			throw new Error(
				`${describe} exited with ${proc.exitCode} during startup${detail(diagnose)}`,
			);
		}
		const taken = await new Promise((resolve) => {
			const socket = dgram.createSocket("udp4");
			socket.once("error", () => resolve(true));
			// No reuseAddr: the whole point is to be refused when the port is in use.
			socket.bind(port, host, () => socket.close(() => resolve(false)));
		});
		if (taken) return;
		if (Date.now() > deadline) {
			throw new Error(
				`${describe} did not bind udp/${host}:${port} within ${timeoutMs}ms${detail(diagnose)}`,
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

module.exports = { ensureCert, ensureCombinedCert, findFreePort, waitForPort, waitForUdpPort };
