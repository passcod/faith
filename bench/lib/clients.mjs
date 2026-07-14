/**
 * Implementation adapters. Every adapter exposes the same shape so the runner
 * measures identical work for each implementation:
 *
 *   makeContext(server, variant)   fresh client state (agent/pool); called per
 *                                  scenario, or per request in cold mode.
 *                                  `variant` may carry agentOptions overrides
 *                                  (fáith only) and a prepare(agent, server)
 *                                  hook.
 *   request(ctx, url, consume)     issue a GET, resolve when response HEADERS
 *                                  are in; returns { status, drain() }
 *                                  resolving when the BODY is done
 *   protocols                      which of h1/h1s/h2/h3 the impl supports
 */

import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { ipv6Available } from "./servers.mjs";

const require = createRequire(import.meta.url);
const rootDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "../..");

export async function loadImpls({ ca }) {
	const impls = new Map();
	const hasV6 = await ipv6Available();

	// Node's built-in fetch (undici). HTTP/1.1 only. Trusting the bench cert
	// requires NODE_EXTRA_CA_CERTS, which the runner sets before re-exec.
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

	// fáith. All protocols.
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
				// Hint h3 so the first request already goes over QUIC, and
				// bind IPv4 where the default IPv6 QUIC socket can't exist.
				options.http3 = {
					hints: [{ host: server.host, port: server.port }],
					...overrides.http3,
				};
				if (!hasV6 && !options.localAddress) {
					options.localAddress = "0.0.0.0";
				}
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
	});

	// node-fetch, if installed. HTTP/1.1 only.
	try {
		const { default: nodeFetch } = await import("node-fetch");
		const https = await import("node:https");
		const http = await import("node:http");
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
		});
	} catch {
		// not installed; skipped
	}

	return impls;
}
