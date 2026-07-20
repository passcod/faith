/**
 * Local benchmark servers with controlled, parameterised behaviour.
 *
 * Routes (all servers):
 *   /payload/<bytes>            respond with <bytes> of deterministic data
 *   /close/payload/<bytes>      same, but with Connection: close (h1 only),
 *                               forcing a fresh connection per request
 *   /delay/<ms>/payload/<bytes> wait <ms> before responding
 *   /cc/<maxage>/payload/<bytes> same, with Cache-Control: public, max-age
 *                               (other routes send no-store)
 *
 * Protocols:
 *   h1   plain HTTP/1.1
 *   h1s  HTTP/1.1 over TLS (self-signed, see ensureCert)
 *   h2   HTTP/2 over TLS (allowHTTP1: false so it's a pure h2 measurement)
 *   h3   HTTP/3, served by the Rust bench/h3-server (Node has no h3
 *        server); built with cargo on first use
 */

import { execFileSync, spawn } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import http from "node:http";
import http2 from "node:http2";
import https from "node:https";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "../..");

const payloadCache = new Map();
function payload(bytes) {
	let buf = payloadCache.get(bytes);
	if (!buf) {
		// Deterministic but incompressible-ish content so compression and
		// caching layers can't skew transfer size between runs. Keep in sync
		// with the same generator in bench/h3-server.
		buf = Buffer.alloc(bytes);
		let seed = 0x2545f491;
		for (let i = 0; i < bytes; i++) {
			seed ^= seed << 13;
			seed ^= seed >>> 17;
			seed ^= seed << 5;
			seed >>>= 0;
			buf[i] = seed & 0xff;
		}
		payloadCache.set(bytes, buf);
	}
	return buf;
}

function parseRoute(url) {
	// Returns { delayMs, bytes, close, maxAge } or null.
	const parts = url.split("?")[0].split("/").filter(Boolean);
	let delayMs = 0;
	let close = false;
	let maxAge = null;
	let i = 0;
	if (parts[i] === "close") {
		close = true;
		i += 1;
	}
	if (parts[i] === "cc") {
		maxAge = Number.parseInt(parts[i + 1], 10);
		if (!Number.isFinite(maxAge) || maxAge < 0) return null;
		i += 2;
	}
	if (parts[i] === "delay") {
		delayMs = Number.parseInt(parts[i + 1], 10);
		if (!Number.isFinite(delayMs) || delayMs < 0) return null;
		i += 2;
	}
	if (parts[i] !== "payload") return null;
	const bytes = Number.parseInt(parts[i + 1], 10);
	if (!Number.isFinite(bytes) || bytes < 0 || bytes > 512 * 1024 * 1024) {
		return null;
	}
	return { delayMs, bytes, close, maxAge };
}

function handle(req, res, route) {
	const body = payload(route.bytes);
	const headers = {
		"content-type": "application/octet-stream",
		"content-length": body.length,
		"cache-control":
			route.maxAge !== null
				? `public, max-age=${route.maxAge}`
				: "no-store",
	};
	if (route.close && !(res.stream || req.httpVersionMajor >= 2)) {
		headers.connection = "close";
	}
	const respond = () => {
		res.writeHead(200, headers);
		res.end(body);
	};
	if (route.delayMs > 0) {
		setTimeout(respond, route.delayMs);
	} else {
		respond();
	}
}

function onRequest(req, res) {
	const route = parseRoute(req.url);
	if (!route) {
		res.writeHead(404, { "content-length": 0 });
		res.end();
		return;
	}
	handle(req, res, route);
}

/**
 * Generate (once, cached in tmp) a private CA plus a leaf certificate for
 * localhost / 127.0.0.1 / ::1. A plain self-signed cert isn't enough: rustls
 * refuses to accept a CA certificate as an end-entity, so servers present the
 * leaf and clients trust the CA.
 *
 * Returns { key, cert, ca, caPath, certPath, keyPath }.
 */
export function ensureCert() {
	// v2: adds the ::1 SAN; the directory name doubles as a cache-buster
	const dir = path.join(os.tmpdir(), "faith-bench-cert-v2");
	const caKeyPath = path.join(dir, "ca-key.pem");
	const caPath = path.join(dir, "ca.pem");
	const keyPath = path.join(dir, "key.pem");
	const csrPath = path.join(dir, "leaf.csr");
	const certPath = path.join(dir, "cert.pem");
	if (!existsSync(caPath) || !existsSync(keyPath) || !existsSync(certPath)) {
		execFileSync("mkdir", ["-p", dir]);
		const ec = ["-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1"];
		execFileSync("openssl", [
			"req", "-x509", ...ec, "-keyout", caKeyPath, "-out", caPath,
			"-days", "7", "-nodes", "-subj", "/CN=faith-bench-ca",
			"-addext", "basicConstraints=critical,CA:TRUE",
		]);
		execFileSync("openssl", [
			"req", "-new", ...ec, "-keyout", keyPath, "-out", csrPath,
			"-nodes", "-subj", "/CN=localhost",
			"-addext",
			"subjectAltName=DNS:localhost,IP:127.0.0.1,IP:0:0:0:0:0:0:0:1",
			"-addext", "basicConstraints=critical,CA:FALSE",
		]);
		execFileSync("openssl", [
			"x509", "-req", "-in", csrPath, "-CA", caPath, "-CAkey", caKeyPath,
			"-CAcreateserial", "-out", certPath, "-days", "7",
			"-copy_extensions", "copyall",
		]);
	}
	return {
		key: readFileSync(keyPath),
		cert: readFileSync(certPath),
		ca: readFileSync(caPath),
		caPath,
		certPath,
		keyPath,
	};
}

/** True if this host can bind IPv6 loopback (cached). */
let v6 = null;
export async function ipv6Available() {
	if (v6 !== null) return v6;
	v6 = await new Promise((resolve) => {
		const probe = net.createServer();
		probe.once("error", () => resolve(false));
		probe.listen(0, "::1", () => {
			probe.close(() => resolve(true));
		});
	});
	return v6;
}

function formatHost(host) {
	return host.includes(":") ? `[${host}]` : host;
}

/**
 * Start a server for the given protocol on an ephemeral port.
 * Returns { url, proto, host, port, close() }.
 */
export async function startServer(proto, host = "127.0.0.1") {
	if (proto === "h3") {
		return startH3Server(host);
	}

	let server;
	let scheme;
	if (proto === "h1") {
		server = http.createServer(onRequest);
		scheme = "http";
	} else if (proto === "h1s") {
		const { key, cert } = ensureCert();
		server = https.createServer({ key, cert }, onRequest);
		scheme = "https";
	} else if (proto === "h2") {
		const { key, cert } = ensureCert();
		server = http2.createSecureServer(
			{ key, cert, allowHTTP1: false },
			onRequest,
		);
		scheme = "https";
	} else {
		throw new Error(`unknown protocol: ${proto}`);
	}

	// Benchmarks intentionally hold many sockets; don't let default timeouts
	// or connection caps interfere with the measurement.
	server.maxConnections = 100_000;
	if ("keepAliveTimeout" in server) server.keepAliveTimeout = 120_000;
	if ("headersTimeout" in server) server.headersTimeout = 120_000;

	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, host, resolve);
	});
	const { port } = server.address();
	return {
		url: `${scheme}://${formatHost(host)}:${port}`,
		proto,
		host,
		port,
		close: () =>
			new Promise((resolve) => {
				server.close(resolve);
				// Don't wait for idle keep-alive sockets.
				server.closeAllConnections?.();
				setTimeout(resolve, 500).unref();
			}),
	};
}

let h3Built = false;
async function startH3Server(host) {
	const { certPath, keyPath } = ensureCert();
	const h3Dir = path.join(rootDir, "bench/h3-server");
	if (!h3Built) {
		// quiet on success, loud on failure
		execFileSync("cargo", ["build", "--release", "--manifest-path", path.join(h3Dir, "Cargo.toml")], {
			stdio: ["ignore", "ignore", "inherit"],
		});
		h3Built = true;
	}
	const bin = path.join(h3Dir, "target/release/h3-server");
	const proc = spawn(
		bin,
		["--cert", certPath, "--key", keyPath, "--port", "0", "--host", host],
		{ stdio: ["ignore", "pipe", "inherit"] },
	);
	const port = await new Promise((resolve, reject) => {
		let buffer = "";
		proc.stdout.on("data", (chunk) => {
			buffer += chunk.toString();
			const m = buffer.match(/LISTENING (\d+)/);
			if (m) resolve(Number(m[1]));
		});
		proc.once("exit", (code) =>
			reject(new Error(`h3-server exited early (code ${code})`)),
		);
	});
	return {
		url: `https://${formatHost(host)}:${port}`,
		proto: "h3",
		host,
		port,
		close: () =>
			new Promise((resolve) => {
				proc.once("exit", resolve);
				proc.kill();
				setTimeout(resolve, 500).unref();
			}),
	};
}
