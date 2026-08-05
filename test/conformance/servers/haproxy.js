/**
 * The HAProxy rows: the only proxy topology in the matrix.
 *
 * Every other row is an origin. This one puts a hop in the middle, which is where
 * the awkward things happen: a chunked body has to be reframed, trailers have to
 * survive being forwarded, and the connection the client sees is not the connection
 * the origin sees. HAProxy also has its own HTTP/2 implementation, neither nghttp2
 * nor Go's.
 *
 * The backend is the controllable HTTP/1 origin rather than go-httpbin, which is what
 * #25 originally proposed: the controllable origin already serves the contract, and
 * go-httpbin does not. That is also where this row's TRAILERS and CHUNKED come from --
 * the pass-through worth testing -- while the static rows can offer neither.
 *
 * Split into HTTP/1 and HTTP/2 frontends for the same reason the other rows are.
 * HTTP/2 has no chunked encoding: a proxy fronting h2 turns the origin's chunked body
 * into DATA frames, so "no Content-Length" becomes true of every response and the
 * framing dimension would pass without observing anything. Only the h1 frontend can
 * show a chunked body surviving a hop.
 */

const { execFileSync, spawn } = require("node:child_process");
const { mkdirSync, writeFileSync } = require("node:fs");
const path = require("node:path");

const { CAPABILITIES: C } = require("../capabilities.js");
const { ensureCombinedCert, findFreePort, waitForPort } = require("../../fixtures/net.js");
const { controllableH1 } = require("./controllable.js");

function buildConf({ port, backendPort, combinedPath, alpn }) {
	// mode http throughout, so HAProxy parses and reframes rather than shifting bytes:
	// a TCP-mode proxy would test nothing about HTTP.
	//
	// Nothing here daemonises, chroots, switches user or opens a stats socket: the
	// foreground is a command-line flag, and this process owns nothing outside its own
	// config. There is also no log directive, because the default target is /dev/log,
	// which need not exist.
	return `global
	maxconn 256

defaults
	mode http
	timeout connect 5s
	timeout client 30s
	timeout server 30s
	option dontlognull

frontend conformance
	bind 127.0.0.1:${port} ssl crt ${combinedPath} alpn ${alpn}
	acl compressible path /encoding/gzip
	use_backend origin_compressed if compressible
	default_backend origin

# Two backends for one origin, because compression in HAProxy is a proxy-level
# setting and everything sharing a backend with it gets compressed. Enabled
# everywhere it stripped Content-Length from every response -- so the framing
# dimension lost the sized case it exists to check -- and it rewrote the origin's
# strong ETag as a weak one, which is correct of HAProxy (the body it sends is a
# different representation) and made every conditional request answer 200 instead
# of 304.
backend origin_compressed
	# HAProxy's own gzip, a third implementation after Go's and nginx's. The offload
	# option strips Accept-Encoding on the way to the backend, so the origin sends plain
	# bytes and what the client decodes was compressed here rather than forwarded --
	# otherwise this row would test the origin's gzip a second time.
	compression algo gzip
	compression type text/plain
	compression offload
	server origin 127.0.0.1:${backendPort} ssl verify none

backend origin
	# The origin presents the test CA's leaf, and verification is deliberately off:
	# this row is about HTTP forwarding, and pinning the CA here would turn a
	# certificate rotation into a proxy failure.
	server origin 127.0.0.1:${backendPort} ssl verify none
`;
}

/** True if a `haproxy` binary is on PATH. */
function haproxyAvailable() {
	try {
		execFileSync("haproxy", ["-v"], { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

function haproxyRow({ name, alpn, expectVersion, capabilities }) {
	return {
		name,
		expectVersion,
		capabilities: new Set(capabilities),
		available: haproxyAvailable,

		async start() {
			const { ca, combinedPath } = ensureCombinedCert();

			// The origin starts first, and only then is the proxy's port chosen. Two
			// processes, two allocations, and findFreePort releases a port before
			// returning it -- so choosing the proxy's first left it unclaimed while the
			// origin picked its own, and the two could land on the same number. The origin
			// bound it, HAProxy could not, and waitForPort saw *a* listener there and
			// called the row ready: every request then went straight to the HTTP/1 origin,
			// which surfaced as "the row negotiates HTTP/2.0" failing about once in eight
			// runs. Allocating after the origin is listening means findFreePort can see
			// that port is taken.
			const origin = await controllableH1.start();
			const backendPort = Number(new URL(origin.url).port);
			const port = await findFreePort();

			const dir = path.join(path.dirname(combinedPath), `haproxy-${port}`);
			mkdirSync(dir, { recursive: true });
			const confPath = path.join(dir, "haproxy.cfg");
			writeFileSync(confPath, buildConf({ port, backendPort, combinedPath, alpn }));

			// -db keeps it in the foreground, so the spawned process is the one serving
			// traffic and killing it is enough. Daemonised, it forks away and holds the
			// port after the run that started it has finished.
			const proc = spawn("haproxy", ["-f", confPath, "-db"], {
				stdio: ["ignore", "pipe", "pipe"],
			});
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
				// The origin goes last: killing it first would leave the proxy answering
				// 503 to anything still in flight.
				await origin.close();
			};

			try {
				await waitForPort({ port, describe: name, proc, diagnose: () => log });
			} catch (err) {
				await close();
				throw err;
			}

			return { url: `https://localhost:${port}`, ca, log: () => log, close };
		},
	};
}

const haproxyH1 = haproxyRow({
	name: "haproxy-h1",
	alpn: "http/1.1",
	expectVersion: "HTTP/1.1",
	capabilities: [
		C.H1,
		C.TLS,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		// Both are the origin's, carried across the hop. This is the only row in the
		// matrix where either has to survive a proxy.
		C.CHUNKED,
		C.TRAILERS,
		// HAProxy's own gzip, with Accept-Encoding stripped on the way to the backend, so
		// what arrives was compressed here rather than forwarded.
		C.GZIP,
		C.SCRIPTABLE,
	],
});

const haproxyH2 = haproxyRow({
	name: "haproxy-h2",
	alpn: "h2,http/1.1",
	expectVersion: "HTTP/2.0",
	capabilities: [
		C.H2,
		C.ALPN_MULTI,
		C.TLS,
		C.CONTENT_LENGTH,
		C.CONDITIONAL,
		C.TRAILERS,
		C.GZIP,
		C.SCRIPTABLE,
	],
});

module.exports = { haproxyH1, haproxyH2, buildConf, haproxyAvailable };
