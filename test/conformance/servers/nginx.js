/**
 * The nginx row.
 *
 * The most-deployed reverse proxy, and in this matrix the point is its *own* gzip
 * module and its own static-file handling -- not its HTTP/2, which is nghttp2, the
 * same implementation Node's h2 rows already exercise.
 *
 * No H3: Ubuntu 24.04 ships nginx 1.24 and HTTP/3 needs 1.25+, so an apt-installed
 * nginx covers HTTP/1 and HTTP/2 only. Declaring H3 here would produce a row that
 * fails everywhere it is deployed rather than one that honestly covers less.
 *
 * No CHUNKED, for the same reason as Caddy: a static file server sends
 * Content-Length, and the only way it chunks is as a side effect of compressing.
 */

const { execFileSync, spawn, spawnSync } = require("node:child_process");
const { mkdirSync, readFileSync, writeFileSync, existsSync } = require("node:fs");
const path = require("node:path");

const { CAPABILITIES: C } = require("../capabilities.js");
const { ensureCert, findFreePort, waitForPort } = require("../../fixtures/net.js");
const { buildStaticTree } = require("./static-tree.js");

/** The largest request header nginx will accept, in bytes. */
const HEADER_LIMIT = 1024;

/**
 * How this nginx spells "enable HTTP/2".
 *
 * `listen ... http2` was deprecated in 1.25.1 in favour of a standalone `http2 on`
 * directive. Both spellings are accepted by exactly one side of that boundary
 * without a warning, and picking the wrong one leaves nginx quietly serving
 * HTTP/1.1 -- which the row's version probe would report as a faith bug. So ask the
 * binary rather than assuming a distribution.
 */
function parseHttp2Style(banner) {
	const match = /nginx\/(\d+)\.(\d+)\.(\d+)/.exec(banner);
	// An unreadable banner takes the modern spelling: the deprecated one is the
	// branch that will eventually be removed, so guessing forward fails on old
	// versions -- which are the ones a version check would have caught anyway --
	// rather than on every future one.
	if (!match) return { directive: "http2 on;", listenSuffix: "", version: null };
	const [major, minor, patch] = match.slice(1).map(Number);
	const modern = major > 1 || (major === 1 && (minor > 25 || (minor === 25 && patch >= 1)));
	return {
		directive: modern ? "http2 on;" : "",
		listenSuffix: modern ? "" : " http2",
		version: `${major}.${minor}.${patch}`,
	};
}

function http2Style(binary) {
	// spawnSync, not execFileSync: `nginx -v` prints its banner to *stderr* and exits
	// 0, and execFileSync returns only stdout -- so the banner came back empty, the
	// parse silently fell through to the modern spelling, and nginx 1.24 rejected the
	// config it was handed. Reading both streams is the whole point of the call.
	const result = spawnSync(binary, ["-v"], { encoding: "utf8" });
	return parseHttp2Style(`${result.stdout || ""}${result.stderr || ""}`);
}

function buildConf({ prefix, tree, port, certPath, keyPath, style }) {
	// Every writable path is inside the prefix. nginx otherwise uses its build-time
	// locations -- /var/log/nginx, /var/lib/nginx -- which an unprivileged test run
	// cannot write, and it fails before it ever binds the port.
	return `daemon off;
worker_processes 1;
error_log ${prefix}/logs/error.log info;
pid ${prefix}/nginx.pid;

events {
	worker_connections 64;
}

http {
	access_log off;
	client_body_temp_path ${prefix}/client_body;
	proxy_temp_path ${prefix}/proxy;
	fastcgi_temp_path ${prefix}/fastcgi;
	uwsgi_temp_path ${prefix}/uwsgi;
	scgi_temp_path ${prefix}/scgi;

	# An empty types block plus a default keeps the config self-contained: the
	# usual "include mime.types" points at a path that moves between distributions,
	# and every file in the tree is text anyway.
	types { }
	default_type text/plain;

	# A request-header ceiling, so this row covers the header-limits dimension. nginx
	# answers an oversized header with 494, which it maps to 400 on the wire.
	large_client_header_buffers 4 ${HEADER_LIMIT/1024}k;

	server {
		listen ${port} ssl${style.listenSuffix};
		${style.directive}
		server_name localhost;

		ssl_certificate ${certPath};
		ssl_certificate_key ${keyPath};

		root ${tree};

		# Scoped to the one route that asks for it. Compressing site-wide would strip
		# Content-Length from every response and the row would stop being able to
		# demonstrate Content-Length framing at all.
		location = /encoding/gzip {
			gzip on;
			gzip_types text/plain;
			# Explicit, so the contract's body size and nginx's default minimum are not
			# quietly load-bearing on each other: below the minimum nginx serves plain
			# bytes, and the encoding dimension would be asserting nothing about gzip.
			gzip_min_length 1;
		}

		# Labels a plain file as gzip: the encoding dimension's negative case.
		location = /encoding/mislabelled {
			add_header Content-Encoding gzip;
		}

		location / {
		}
	}
}
`;
}

const nginx = {
	name: "nginx",
	expectVersion: "HTTP/2.0",
	capabilities: new Set([
		C.H1,
		C.H2,
		C.ALPN_MULTI,
		C.TLS,
		C.GZIP,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		C.HEADER_LIMITS,
		C.SCRIPTABLE,
	]),
	headerLimit: HEADER_LIMIT,

	available() {
		try {
			execFileSync("nginx", ["-v"], { stdio: "ignore" });
			return true;
		} catch {
			return false;
		}
	},

	async start() {
		const { ca, certPath, keyPath } = ensureCert();
		const port = await findFreePort();
		const tree = buildStaticTree();
		const prefix = path.join(tree.dir, "nginx");
		// `logs/` must exist before nginx starts: it opens its compiled-in default
		// error log relative to the prefix *before* parsing the config that redirects
		// it, so a missing directory is an alert on every startup -- and the config's
		// own error_log then lands in the same place anyway.
		mkdirSync(path.join(prefix, "logs"), { recursive: true });

		const confPath = path.join(prefix, "nginx.conf");
		writeFileSync(
			confPath,
			buildConf({
				prefix,
				tree: tree.dir,
				port,
				certPath,
				keyPath,
				style: http2Style("nginx"),
			}),
		);

		const readLog = () => {
			const log = path.join(prefix, "logs", "error.log");
			return existsSync(log) ? readFileSync(log, "utf8") : "";
		};

		const proc = spawn("nginx", ["-p", prefix, "-c", confPath], {
			stdio: ["ignore", "pipe", "pipe"],
		});
		let stderr = "";
		proc.stderr.on("data", (c) => {
			stderr += c.toString();
		});

		try {
			await waitForPort({
				port,
				describe: "nginx",
				proc,
				diagnose: () => `${stderr}${readLog()}`,
			});
		} catch (err) {
			proc.kill();
			throw err;
		}

		return {
			url: `https://localhost:${port}`,
			ca,
			log: readLog,
			close: async () => {
				proc.kill();
				// Wait for it to actually go, so the next row can rebind the port. nginx
				// is spawned in the foreground, so its exit is the whole process tree's.
				await new Promise((resolve) => {
					if (proc.exitCode !== null) return resolve();
					proc.once("exit", resolve);
					setTimeout(resolve, 2_000).unref();
				});
				tree.cleanup();
			},
		};
	},
};

module.exports = { nginx, buildConf, http2Style, parseHttp2Style, HEADER_LIMIT };
