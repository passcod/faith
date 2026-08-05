/**
 * The Apache httpd row.
 *
 * Here for conservative HTTP/1 semantics and for the two connection-lifecycle
 * behaviours nothing else in the matrix offers as configuration:
 * `MaxKeepAliveRequests`, which makes the server close a connection after a known
 * number of requests, and `LimitRequestFieldSize`, which makes it reject an
 * oversized request header. Both are exactly what a client's connection handling
 * has to survive, and neither is scriptable in the Node origin without
 * reimplementing a server.
 *
 * Its HTTP/2 is nghttp2 -- the same implementation the Node h2 row uses -- so this
 * row does not earn its place on HTTP/2 diversity, and nothing here pretends it
 * does.
 */

const { execFileSync, spawn, spawnSync } = require("node:child_process");
const { existsSync, mkdirSync, readFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");

const { CAPABILITIES: C } = require("../capabilities.js");
const { ensureCert, findFreePort, waitForPort } = require("../../fixtures/net.js");
const { buildStaticTree } = require("./static-tree.js");

/** How many requests a connection serves before Apache closes it. */
const KEEPALIVE_LIMIT = 2;

/** The largest request header Apache will accept, in bytes. */
const HEADER_LIMIT = 1024;

/**
 * The binary is `httpd` on Arch and Red Hat, `apache2` on Debian and Ubuntu.
 *
 * Returned as a name rather than a path so the spawn inherits PATH resolution, and
 * so the error message names both candidates instead of just the last one tried.
 */
function locateBinary() {
	for (const candidate of ["httpd", "apache2"]) {
		try {
			execFileSync(candidate, ["-v"], { stdio: "ignore" });
			return candidate;
		} catch {}
	}
	return null;
}

/**
 * Where the LoadModule lines point.
 *
 * There is no portable way to ask httpd this: `-V` reports HTTPD_ROOT and the
 * config path but not the module directory, and `apxs` is in a separate -dev
 * package that a test runner should not require. So the known layouts are tried and
 * confirmed by looking for a module that must exist, and a miss lists everything it
 * looked at -- a wrong guess here otherwise surfaces as "cannot load mod_ssl.so",
 * which does not say where it looked.
 */
const MODULE_DIRS = [
	"/usr/lib/apache2/modules", // Debian, Ubuntu
	"/usr/lib/httpd/modules", // Arch
	"/usr/lib64/httpd/modules", // Fedora, RHEL
	"/usr/libexec/apache2", // macOS
];

function locateModules() {
	for (const dir of MODULE_DIRS) {
		if (existsSync(path.join(dir, "mod_ssl.so"))) return dir;
	}
	return null;
}

/**
 * Modules loaded explicitly, because a distribution's own httpd.conf is not used --
 * the config here is self-contained so the row behaves the same everywhere.
 *
 * `filter` is not optional despite nothing naming it: mod_deflate registers its
 * filter through mod_filter, and without it `SetOutputFilter DEFLATE` is accepted
 * and then does nothing, which would leave the encoding dimension asserting gzip
 * behaviour about an uncompressed body.
 *
 * mod_mime is deliberately absent. It refuses to start without a TypesConfig file
 * resolved against ServerRoot, and it has nothing to contribute: the contract's
 * paths have no extensions to map, and ForceType below -- which is core -- gives
 * them the one type this row needs.
 */
const MODULES = [
	["mpm_event_module", "mod_mpm_event.so"],
	["unixd_module", "mod_unixd.so"],
	["authz_core_module", "mod_authz_core.so"],
	["log_config_module", "mod_log_config.so"],
	["alias_module", "mod_alias.so"],
	["filter_module", "mod_filter.so"],
	["deflate_module", "mod_deflate.so"],
	["headers_module", "mod_headers.so"],
	["ssl_module", "mod_ssl.so"],
	["http2_module", "mod_http2.so"],
];

/**
 * Modules this build compiled in, as the file names they would otherwise load from.
 *
 * Which modules are static is a packaging decision: Debian compiles mod_unixd in,
 * Arch ships it as a shared object, and LoadModule for a static one is a hard error
 * -- "module unixd_module is built-in and can't be loaded" -- so httpd never starts.
 * Asking the binary is the only way to know, and -l is what answers.
 */
function builtinModules(binary) {
	const result = spawnSync(binary, ["-l"], { encoding: "utf8" });
	const names = `${result.stdout || ""}${result.stderr || ""}`.match(/mod_\w+\.c/g) || [];
	return new Set(names.map((name) => name.replace(/\.c$/, ".so")));
}

function buildConf({ prefix, tree, port, certPath, keyPath, modules, protocols, builtin }) {
	return `ServerName localhost
ServerRoot "${prefix}"
PidFile "${prefix}/httpd.pid"
ErrorLog "${prefix}/error.log"
LogLevel info
# Everything httpd writes at runtime goes in the prefix. The compiled-in default is
# somewhere like /run/httpd, which an unprivileged run cannot create.
DefaultRuntimeDir "${prefix}"
# Semaphores rather than lock files, for the same reason: the default mutex
# mechanism wants a directory this run does not own.
Mutex sem

${MODULES.filter(([, file]) => !builtin.has(file) && existsSync(path.join(modules, file)))
	.map(([name, file]) => `LoadModule ${name} "${path.join(modules, file)}"`)
	.join("\n")}

Listen ${port}
Protocols ${protocols}

# The row's reason for existing: a connection the server closes after a known
# number of requests, and a request header size it refuses.
MaxKeepAliveRequests ${KEEPALIVE_LIMIT}
KeepAlive On
KeepAliveTimeout 15
LimitRequestFieldSize ${HEADER_LIMIT}

SSLEngine on
SSLCertificateFile "${certPath}"
SSLCertificateKeyFile "${keyPath}"
# No session cache, so mod_socache_shmcb need not be loaded or given a writable
# path. Nothing here reuses a session.
SSLSessionCache none

DocumentRoot "${tree}"

<Directory "${tree}">
	Require all granted
	# The contract's paths have no file extensions, so mod_mime infers no type --
	# and mod_deflate's filter only ever sees a typed response. Without this the
	# encoding route is served uncompressed.
	ForceType text/plain
</Directory>

# Compression scoped to the one route that asks for it. Site-wide it would strip
# Content-Length from every response, and the row could no longer demonstrate
# Content-Length framing at all.
<Location "/encoding/gzip">
	SetOutputFilter DEFLATE
</Location>

# Labels a plain file as gzip: the encoding dimension's negative case.
<Location "/encoding/mislabelled">
	Header set Content-Encoding gzip
</Location>
`;
}

/**
 * Split into HTTP/1-only and HTTP/2-preferring rows, for the same reason the Node
 * origin is: `MaxKeepAliveRequests` is an HTTP/1 keepalive limit and HTTP/2 ignores
 * it entirely -- six requests over an h2 connection to a server configured to close
 * after two all succeed on the same connection, so the keepalive dimension would
 * assert nothing there. Splitting keeps that capability on the row that actually
 * has it.
 */
function apacheRow({ name, protocols, expectVersion, capabilities }) {
	return {
		name,
		expectVersion,
		capabilities: new Set(capabilities),
		// Published so the keepalive dimension can ask for more requests than the limit
		// without hardcoding a number that would silently stop being a limit if changed.
		keepaliveLimit: KEEPALIVE_LIMIT,
		headerLimit: HEADER_LIMIT,

		available() {
			return locateBinary() !== null && locateModules() !== null;
		},

		async start() {
			const binary = locateBinary();
			const modules = locateModules();
			if (!binary) throw new Error("neither httpd nor apache2 is on PATH");
			if (!modules) {
				throw new Error(`no Apache module directory found; looked in ${MODULE_DIRS.join(", ")}`);
			}

			const { ca, certPath, keyPath } = ensureCert();
			const port = await findFreePort();
			const tree = buildStaticTree();
			const prefix = path.join(tree.dir, "apache");
			mkdirSync(prefix, { recursive: true });

			const confPath = path.join(prefix, "httpd.conf");
			writeFileSync(
				confPath,
				buildConf({
					prefix,
					tree: tree.dir,
					port,
					certPath,
					keyPath,
					modules,
					protocols,
					builtin: builtinModules(binary),
				}),
			);

			const readLog = () => {
				const log = path.join(prefix, "error.log");
				return existsSync(log) ? readFileSync(log, "utf8") : "";
			};

			// -X: one worker, in the foreground. Otherwise httpd daemonises, the spawned
			// process exits immediately, and there is nothing left to kill -- the server
			// would outlive the test run.
			const proc = spawn(binary, ["-f", confPath, "-X"], { stdio: ["ignore", "pipe", "pipe"] });
			let stderr = "";
			proc.stderr.on("data", (c) => {
				stderr += c.toString();
			});

			try {
				await waitForPort({
					port,
					describe: binary,
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
}

const apacheH1 = apacheRow({
	name: "apache-h1",
	protocols: "http/1.1",
	expectVersion: "HTTP/1.1",
	capabilities: [
		C.H1,
		C.TLS,
		C.GZIP,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		// Both of these are why the row is here, and both are HTTP/1 semantics.
		C.KEEPALIVE_LIMIT,
		C.HEADER_LIMITS,
		C.SCRIPTABLE,
	],
});

const apacheH2 = apacheRow({
	name: "apache-h2",
	protocols: "h2 http/1.1",
	expectVersion: "HTTP/2.0",
	// No H1, though http/1.1 is offered: a dimension needing HTTP/1 must not run on a
	// row that negotiates h2. ALPN_MULTI is how "offers both" is declared.
	capabilities: [
		C.H2,
		C.ALPN_MULTI,
		C.TLS,
		C.GZIP,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		C.HEADER_LIMITS,
		C.SCRIPTABLE,
	],
});

module.exports = {
	apacheH1,
	builtinModules,
	apacheH2,
	buildConf,
	locateBinary,
	locateModules,
	KEEPALIVE_LIMIT,
	HEADER_LIMIT,
};
