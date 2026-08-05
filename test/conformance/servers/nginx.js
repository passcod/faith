/**
 * The nginx row.
 *
 * The most-deployed reverse proxy, here for its own gzip module, its own static-file
 * handling, and its own QUIC -- which is neither quiche nor quic-go, so this is a
 * fourth HTTP/3 stack in the matrix. Not for its HTTP/2, which is nghttp2, the same
 * implementation the Node h2 row already exercises.
 *
 * It is also the only row whose Alt-Svc header is written by hand, because nginx
 * advertises nothing by itself. That is closer to how most deployments look than
 * Caddy's automatic advertisement, and it is a second, differently-produced path
 * through the upgrade code #23 broke.
 *
 * HTTP/3 is a compile-time module, so this row needs a build configured with
 * --with-http_v3_module: Ubuntu 24.04's 1.24 cannot have it at all. Rather than split
 * into two rows -- which would duplicate eight identical TCP cells to gain two -- the
 * whole row reports itself unavailable on a build without QUIC, and says so.
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
 * The oldest nginx this row can use: 1.25.1.
 *
 * Two boundaries land three weeks apart. HTTP/3 arrived in 1.25.0, and 1.25.1 added
 * the standalone `http2` directive, deprecating the `listen ... http2` parameter.
 * Since the row needs QUIC, only 1.25.0 itself could want the old spelling -- one
 * release, with HTTP/3 still marked experimental -- so the minimum is 1.25.1 and the
 * config speaks one dialect. Handling both would be a branch reachable by nothing
 * anyone runs.
 */
const MINIMUM = [1, 25, 1];

/** The version from an `nginx -v` banner, or null if it does not look like one. */
function parseVersion(banner) {
	const match = /nginx\/(\d+)\.(\d+)\.(\d+)/.exec(banner);
	return match ? match.slice(1).map(Number) : null;
}

function newEnough(version) {
	if (!version) return false;
	for (const [index, floor] of MINIMUM.entries()) {
		if (version[index] > floor) return true;
		if (version[index] < floor) return false;
	}
	return true;
}

/**
 * The version banner. spawnSync, not execFileSync: `nginx -v` prints to *stderr* and
 * exits 0, and execFileSync returns only stdout -- so reading it that way yielded an
 * empty string and every version looked the same.
 */
function versionBanner(binary) {
	const result = spawnSync(binary, ["-v"], { encoding: "utf8" });
	return `${result.stdout || ""}${result.stderr || ""}`;
}

function buildConf({ prefix, tree, port, certPath, keyPath }) {
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
		listen ${port} ssl;
		http2 on;
${h3ServerConf(port)}		server_name localhost;

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

/**
 * The QUIC half of the config.
 *
 * A second `listen` on the same port number, because one is UDP and the other TCP.
 * `reuseport` is what lets nginx spread QUIC connections across workers, and may
 * appear only once per port in a configuration -- there is a single server block
 * here, so that is fine, and it is what nginx's own documentation uses.
 *
 * The Alt-Svc header is written out by hand, which is the interesting part: nginx
 * emits no advertisement of its own, unlike Caddy. So this row exercises the upgrade
 * path against a header someone wrote themselves, which is what most real
 * deployments have. `always` so it is sent on error responses too, and the port is
 * interpolated rather than left to $server_port, which is the TCP port and only
 * happens to match.
 */
function h3ServerConf(port) {
	return `		listen ${port} quic reuseport;
		http3 on;
		add_header Alt-Svc 'h3=":${port}"; ma=86400' always;
`;
}

/** What this nginx was built with. `-V` prints its configure line, to stderr. */
function buildFlags(binary) {
	const result = spawnSync(binary, ["-V"], { encoding: "utf8" });
	return `${result.stdout || ""}${result.stderr || ""}`;
}

const nginx = {
	name: "nginx",
	expectVersion: "HTTP/2.0",
	capabilities: new Set([
		C.H1,
		C.H2,
		C.H3,
		C.ALTSVC,
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
		} catch {
			return false;
		}
		// Checked here rather than folded into the capability declarations, which have to
		// stay static for the computed matrix to be identical on every machine. A binary
		// that is too old, or built without QUIC, makes the row unavailable instead.
		return (
			newEnough(parseVersion(versionBanner("nginx"))) &&
			buildFlags("nginx").includes("--with-http_v3_module")
		);
	},

	/**
	 * Why the row did not run, when it did not.
	 *
	 * "not installed" would be a lie in the common case: nginx is there, built without
	 * QUIC. Someone chasing a missing row needs to know which, because the two fixes
	 * have nothing to do with each other.
	 */
	whyUnavailable() {
		try {
			execFileSync("nginx", ["-v"], { stdio: "ignore" });
		} catch {
			return "nginx is not installed";
		}
		const version = parseVersion(versionBanner("nginx"));
		if (!newEnough(version)) {
			return `this nginx is ${version ? version.join(".") : "of an unreadable version"}, ` +
				`and the row needs ${MINIMUM.join(".")} or newer`;
		}
		return "this nginx was built without --with-http_v3_module";
	},

	async start() {
		const { ca, certPath, keyPath } = ensureCert();
		const port = await findFreePort();
		const tree = buildStaticTree();
		const prefix = path.join(tree.dir, "nginx");
		// `logs/` must exist before nginx starts: it opens its compiled-in default error
		// log relative to the prefix *before* parsing the config that redirects it, so a
		// missing directory is an alert on every startup -- and the config's own
		// error_log then lands in the same place anyway.
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
				// Wait for it to actually go, so the next row can rebind the port. nginx is
				// spawned in the foreground, so its exit is the whole process tree's.
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

module.exports = { nginx, buildConf, parseVersion, newEnough, MINIMUM, HEADER_LIMIT };
