/**
 * Implementation adapters. Every adapter exposes the same shape so the runner
 * measures identical work for each implementation:
 *
 *   protocols                      which of h1/h1s/h2/h3 the impl supports
 *   makeContext(server, variant)   fresh client state (agent/pool/session);
 *                                  called per scenario, or per request in cold
 *                                  mode. `variant` may carry agentOptions
 *                                  overrides (Faith only) and a
 *                                  prepare(agent, server) hook.
 *   request(ctx, url, consume)     issue a GET, resolve when response HEADERS
 *                                  are in; returns { status, drain() }
 *                                  resolving when the BODY is done
 *   closeContext(ctx)              optional teardown (close pools/sessions)
 *
 * The clients cover three transport stacks: undici's own engine (native
 * fetch, undici.request), node's libuv http core (node:http2, got,
 * node-fetch), and native bindings (Faith → reqwest/hyper, libcurl). Two
 * wrappers over the same stack mostly measure wrapper overhead, so the set is
 * chosen to have at most one representative per (stack, API-style).
 *
 * ttfb vs drain: streaming clients resolve at response headers, so ttfb is a
 * true time-to-headers. Buffering clients (libcurl's curly) read the whole
 * body before resolving; their ttfb equals total and should be read as such.
 */

import http from "node:http";
import http2 from "node:http2";
import https from "node:https";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const rootDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "../..");

export async function loadImpls({ ca }) {
	const impls = new Map();

	// --- undici, two ways ------------------------------------------------

	// Node's built-in fetch (undici's standard fetch wrapper). HTTP/1.1 only. Trusting
	// the bench CA requires NODE_EXTRA_CA_CERTS, which the runner sets before
	// re-exec, so no per-context config is needed here.
	impls.set("native", {
		name: "native",
		protocols: ["h1", "h1s"],
		makeContext: () => ({}),
		request: async (_ctx, url, consume) => {
			const response = await fetch(url);
			return {
				status: response.status,
				drain: async () => {
					if (consume === "text") return void (await response.text());
					return void (await response.bytes());
				},
			};
		},
	});

	// undici's raw request() API — the same engine as native fetch without the
	// WHATWG streams and standard fetch machinery, i.e. the true ceiling of the undici stack.
	const undici = require("undici");
	impls.set("undici", {
		name: "undici",
		protocols: ["h1", "h1s"],
		makeContext: () => new undici.Agent({ connect: { ca } }),
		request: async (dispatcher, url, consume) => {
			const { statusCode, body } = await undici.request(url, { dispatcher });
			return {
				status: statusCode,
				drain: async () => {
					if (consume === "text") return void (await body.text());
					return void (await body.arrayBuffer());
				},
			};
		},
		closeContext: (dispatcher) => dispatcher.close(),
	});

	// --- node libuv http core -------------------------------------------

	// node:http2 core client. h2 only, and the only builtin h2 client — a
	// raw-core baseline with no third-party wrapper.
	impls.set("http2", {
		name: "http2",
		protocols: ["h2"],
		makeContext: (server) => {
			// Match the server's raised limits so the client session doesn't hit
			// its own 10 MB memory cap under concurrent large streams.
			const session = http2.connect(server.url, {
				ca,
				maxSessionMemory: 1024,
			});
			session.on("error", () => {}); // avoid crashing on teardown races
			const ready = new Promise((resolve, reject) => {
				session.once("connect", resolve);
				session.once("error", reject);
			});
			return { session, ready };
		},
		request: async (ctx, url, consume) => {
			await ctx.ready;
			const req = ctx.session.request({ ":path": new URL(url).pathname });
			const status = await new Promise((resolve, reject) => {
				req.once("response", (h) => resolve(h[":status"]));
				req.once("error", reject);
			});
			return {
				status,
				drain: () =>
					new Promise((resolve, reject) => {
						const chunks = [];
						req.on("data", (c) => chunks.push(c));
						req.once("end", () => {
							if (consume === "text") Buffer.concat(chunks).toString("utf8");
							resolve();
						});
						req.once("error", reject);
					}),
			};
		},
		closeContext: (ctx) => ctx.session.close(),
	});

	// got. h1/h1s over node core, plus h2 via http2-wrapper — the popular JS
	// client that actually speaks HTTP/2, so it gives the h2 rows a realistic
	// (non-raw-core) competitor.
	const { default: got } = await import("got");
	impls.set("got", {
		name: "got",
		protocols: ["h1", "h1s", "h2"],
		makeContext: (server) => {
			const h2 = server.proto === "h2";
			return got.extend({
				http2: h2,
				https: { certificateAuthority: ca },
				retry: { limit: 0 },
				// keep-alive agents for the h1 stacks; http2-wrapper pools h2 itself
				agent: h2
					? undefined
					: {
							http: new http.Agent({ keepAlive: true }),
							https: new https.Agent({ keepAlive: true, ca }),
						},
			});
		},
		request: (client, url, consume) => {
			const p = client(url, {
				responseType: consume === "text" ? "text" : "buffer",
			});
			return new Promise((resolve, reject) => {
				p.once("response", (res) => {
					resolve({
						status: res.statusCode,
						drain: () => p.then(() => {}, reject),
					});
				});
				p.catch(reject);
			});
		},
		closeContext: (client) => {
			const a = client.defaults.options.agent;
			a?.http?.destroy?.();
			a?.https?.destroy?.();
		},
	});

	// node-fetch. HTTP/1.1 over node core; a standard-shaped wrapper, unlike got.
	const { default: nodeFetch } = await import("node-fetch");
	impls.set("node-fetch", {
		name: "node-fetch",
		protocols: ["h1", "h1s"],
		makeContext: () => ({
			httpAgent: new http.Agent({ keepAlive: true }),
			httpsAgent: new https.Agent({ keepAlive: true, ca }),
		}),
		request: async (ctx, url, consume) => {
			const response = await nodeFetch(url, {
				agent: (parsed) =>
					parsed.protocol === "https:" ? ctx.httpsAgent : ctx.httpAgent,
			});
			return {
				status: response.status,
				drain: async () => {
					if (consume === "text") return void (await response.text());
					return void (await response.arrayBuffer());
				},
			};
		},
		closeContext: (ctx) => {
			ctx.httpAgent.destroy();
			ctx.httpsAgent.destroy();
		},
	});

	// --- native bindings -------------------------------------------------

	// libcurl (node-libcurl). Faith's closest architectural cousin: the other
	// native-bindings client, so it answers whether Faith's NAPI overhead is
	// competitive with libcurl's. curly buffers the whole body, so its ttfb
	// equals total.
	//
	// Peer verification is disabled: this node-libcurl prebuilt statically
	// links its own OpenSSL with a baked CA path and ignores CAINFO,
	// CURL_CA_BUNDLE and SSL_CERT_FILE, so a private-CA bench cert cannot be
	// trusted portably. The full TLS handshake and record crypto still run;
	// only the (per-connection, keep-alive-amortized) chain check is skipped.
	// node-libcurl ships a native addon and is the one client that may not
	// build on a given platform (it's an optionalDependency for that reason).
	// If its addon isn't loadable, skip only libcurl — loudly, so a missing
	// row is never mistaken for a silent opt-out — and leave the rest intact.
	try {
		const { curly, CurlHttpVersion, Share, CurlShareOption, CurlShareLock } =
			require("node-libcurl");
		impls.set("libcurl", {
			name: "libcurl",
			protocols: ["h1", "h1s", "h2"],
			makeContext: (server) => {
				// a Share handle lets pooled connections and TLS sessions be reused
				// across curly calls (curly uses a fresh easy handle each call)
				const share = new Share();
				share.setOpt(CurlShareOption.Share, CurlShareLock.DataConnect);
				share.setOpt(CurlShareOption.Share, CurlShareLock.DataSslSession);
				const httpVersion =
					server.proto === "h2" ? CurlHttpVersion.V2Tls : CurlHttpVersion.V1_1;
				return { share, httpVersion };
			},
			request: async (ctx, url, consume) => {
				const { statusCode, data } = await curly.get(url, {
					share: ctx.share,
					httpVersion: ctx.httpVersion,
					sslVerifyPeer: false,
					sslVerifyHost: false,
					curlyResponseBodyParser: false,
				});
				return {
					status: statusCode,
					drain: async () => {
						if (consume === "text") data.toString("utf8");
					},
				};
			},
			closeContext: (ctx) => ctx.share.close(),
		});
	} catch (err) {
		console.error(
			`  note: skipping libcurl — node-libcurl not loadable (${err.message})`,
		);
	}

	// Faith. All protocols.
	const faith = require(path.join(rootDir, "wrapper.js"));
	impls.set("faith", {
		name: "faith",
		protocols: ["h1", "h1s", "h2", "h3"],
		makeContext: (server, variant = {}) => {
			const overrides = variant.agentOptions ?? {};
			const options = {
				...overrides,
				tls: { extraRoots: [ca.toString()], ...overrides.tls },
			};
			if (server?.proto === "h3") {
				// Hint h3 so the first request already goes over QUIC. The agent
				// binds IPv4 by itself where the default IPv6 QUIC socket can't
				// exist, so no explicit localAddress is needed here.
				options.http3 = {
					hints: [{ host: server.host, port: server.port }],
					...overrides.http3,
				};
			}
			const agent = new faith.Agent(options);
			variant.prepare?.(agent, server);
			return agent;
		},
		request: async (agent, url, consume) => {
			const response = await faith.fetch(url, { agent });
			return {
				status: response.status,
				drain: async () => {
					if (consume === "discard") return void (await response.discard());
					if (consume === "text") return void (await response.text());
					return void (await response.bytes());
				},
			};
		},
		// Free the pool/resolver deterministically in cold mode instead of
		// waiting for GC; otherwise fresh-per-request agents pile up. Optional
		// chaining so an older addon without close() still runs.
		closeContext: (agent) => agent.close?.(),
	});

	return impls;
}
