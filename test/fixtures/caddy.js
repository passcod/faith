/**
 * Spawning Caddy, shared by the HTTP/3 regression tests and the conformance row.
 *
 * Lives here rather than in either caller because both need a real h1/h2/h3
 * server over the test CA, and the startup handshake -- watch the log for
 * "serving initial configuration", kill on timeout -- is the fiddly part worth
 * having once.
 */

const { execFileSync, spawn } = require("node:child_process");
const { writeFileSync } = require("node:fs");
const path = require("node:path");

const { ensureCert } = require("./net.js");

/** How long a Caddy gets to go down politely before it is put down. */
const SHUTDOWN_GRACE_MS = 2000;

/**
 * Stop a Caddy and resolve once it is actually gone.
 *
 * `kill()` alone only asks: Caddy shuts down gracefully and can outlive the signal,
 * and a live child with piped stdio holds the event loop open, so a run whose Caddy
 * lingers cannot end. That surfaces as a whole test run hanging long after the last
 * assertion, so the signal is escalated rather than trusted.
 */
function stopCaddy(proc) {
	return new Promise((resolve) => {
		if (proc.exitCode !== null || proc.signalCode !== null) {
			resolve();
			return;
		}

		// Unref'd because the child itself is holding the loop: while there is anything
		// left to wait for, this timer still fires, and once there isn't, it is moot.
		const force = setTimeout(() => proc.kill("SIGKILL"), SHUTDOWN_GRACE_MS);
		force.unref();
		proc.once("exit", () => {
			clearTimeout(force);
			resolve();
		});
		proc.kill();
	});
}

/**
 * Spawn Caddy serving https://localhost:<port> over h1/h2/h3.
 *
 * `altSvc` overrides the Alt-Svc header Caddy would emit for itself, so a test can
 * advertise an endpoint that isn't Caddy — a port with nothing listening, say.
 *
 * `cacheControl` adds a Cache-Control header, so responses become storable by an
 * agent configured with a cache store.
 *
 * `directives` replaces what the site block does. The default answers everything
 * with a fixed string, which is all the HTTP/3 tests need; the conformance row
 * passes a file server and its own per-path behaviours instead.
 */
async function startCaddy({
	port,
	dir,
	altSvc,
	cacheControl,
	directives = [`respond "hello-from-caddy"`],
}) {
	const { certPath, keyPath } = ensureCert();
	// Backtick-quoted, because these values contain the double quotes Caddy would
	// otherwise treat as the end of the token.
	const lines = [`tls ${certPath} ${keyPath}`];
	if (altSvc) lines.push(`header Alt-Svc \`${altSvc}\``);
	if (cacheControl) lines.push(`header Cache-Control \`${cacheControl}\``);
	lines.push(...directives);

	const caddyfile = `{
	auto_https off
	admin off
	servers {
		protocols h1 h2 h3
	}
}

https://localhost:${port} {
${lines.map((d) => `\t${d}`).join("\n")}
}
`;
	const configPath = path.join(dir, `Caddyfile.${port}`);
	writeFileSync(configPath, caddyfile);

	const proc = spawn("caddy", ["run", "--config", configPath, "--adapter", "caddyfile"], {
		stdio: ["ignore", "pipe", "pipe"],
	});
	let log = "";
	proc.stdout.on("data", (c) => {
		log += c.toString();
	});
	proc.stderr.on("data", (c) => {
		log += c.toString();
	});

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
				stopCaddy(proc);
				reject(new Error(`caddy failed to start:\n${log}`));
			}
		}, 100);
	});

	return { port, log: () => log, close: () => stopCaddy(proc) };
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

module.exports = { startCaddy, caddyAvailable };
