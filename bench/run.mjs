#!/usr/bin/env node
/**
 * Faith benchmark runner.
 *
 * Principles (see bench/README.md for the full methodology):
 *  - measure inside a long-lived process; process/module startup is excluded
 *  - local servers with controlled payloads; the network is not the variable
 *  - warmup samples are discarded; distributions (p50/p90/p99) are reported,
 *    never just totals
 *  - time-to-first-byte (headers) and body drain are measured separately
 *  - identical body consumption across implementations by default
 *  - event-loop delay is recorded while the scenario runs
 *
 * Usage:
 *   node bench/run.mjs [--suite quick|full|concurrency|features]
 *     [--impls native,faith,node-fetch] [--protos h1,h1s,h2,h3]
 *     [--sizes 0,1024,65536,1048576] [--conc 1,16,64] [--modes warm,cold]
 *     [--consume bytes|text|discard] [--samples 200] [--warmup 50]
 *     [--delay 0] [--out bench/results]
 *
 * The `features` suite benchmarks Faith against itself across its feature
 * set: HTTP versions, DNS resolvers, IPv4/IPv6, HTTP caching, cookies.
 */

import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, createWriteStream } from "node:fs";
import os from "node:os";
import path from "node:path";
import { monitorEventLoopDelay, performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

import { loadImpls } from "./lib/clients.mjs";
import { ensureCert, ipv6Available, startServer } from "./lib/servers.mjs";
import { fmtMs, summarise } from "./lib/stats.mjs";

const args = process.argv.slice(2);
function flag(name, fallback) {
	const idx = args.indexOf(`--${name}`);
	if (idx === -1 || idx + 1 >= args.length) return fallback;
	return args[idx + 1];
}
function listFlag(name, fallback) {
	const raw = flag(name, null);
	return raw === null ? fallback : raw.split(",").filter(Boolean);
}

const suite = flag("suite", "quick");
const defaults =
	suite === "full"
		? {
				impls: [
					"native",
					"undici",
					"http2",
					"got",
					"node-fetch",
					"libcurl",
					"faith",
				],
				protos: ["h1", "h1s", "h2", "h3"],
				sizes: [0, 1024, 65536, 1048576],
				conc: [1, 16, 64],
				modes: ["warm", "cold"],
				samples: 500,
				warmup: 100,
			}
		: suite === "concurrency"
			? {
					// A concurrency sweep: same clients as `full` but many more
					// concurrency points, so the throughput-vs-concurrency curve
					// has real shape. Warm only, fewer sizes, to keep it tractable.
					impls: [
						"native",
						"undici",
						"http2",
						"got",
						"node-fetch",
						"libcurl",
						"faith",
					],
					protos: ["h1", "h1s", "h2", "h3"],
					sizes: [1024, 65536],
					conc: [1, 4, 8, 16, 24, 32, 48, 64, 128],
					modes: ["warm"],
					samples: 300,
					warmup: 60,
				}
			: suite === "features"
			? {
					impls: ["faith"],
					protos: [], // driven by the variant list instead
					sizes: [1024, 65536],
					conc: [1, 16],
					modes: ["warm"],
					samples: 200,
					warmup: 50,
				}
			: {
					impls: ["native", "faith"],
					protos: ["h1", "h2"],
					sizes: [1024, 65536],
					conc: [1, 16],
					modes: ["warm"],
					samples: 200,
					warmup: 50,
				};

const impls = listFlag("impls", defaults.impls);
const protos = listFlag("protos", defaults.protos);
const sizes = listFlag("sizes", defaults.sizes.map(String)).map(Number);
const concurrencies = listFlag("conc", defaults.conc.map(String)).map(Number);
const modes = listFlag("modes", defaults.modes);
const consume = flag("consume", "bytes");
const samples = Number(flag("samples", defaults.samples));
const warmup = Number(flag("warmup", defaults.warmup));
const delayMs = Number(flag("delay", 0));
const outDir = flag(
	"out",
	path.join(path.dirname(fileURLToPath(import.meta.url)), "results"),
);

// TLS protocols require the bench CA to be trusted by Node's own fetch, which
// can only be configured through the environment; re-exec once with it set.
const { caPath, ca } = ensureCert();
if (
	(protos.some((p) => p !== "h1") || suite === "features") &&
	process.env.NODE_EXTRA_CA_CERTS !== caPath &&
	!process.env.FAITH_BENCH_REEXEC
) {
	const result = spawnSync(process.execPath, process.argv.slice(1), {
		stdio: "inherit",
		env: {
			...process.env,
			NODE_EXTRA_CA_CERTS: caPath,
			FAITH_BENCH_REEXEC: "1",
		},
	});
	process.exit(result.status ?? 1);
}

mkdirSync(outDir, { recursive: true });
const stamp = new Date().toISOString().replace(/[:.]/g, "-");
const outPath = path.join(outDir, `bench-${stamp}.ndjson`);
const out = createWriteStream(outPath);

const implementations = await loadImpls({ ca });

/** Reject if `promise` hasn't settled within `ms`, so a wedged request can't
 * hang the run. Clears its timer either way so it never keeps the loop alive. */
function withTimeout(promise, ms, label) {
	let timer;
	const guard = new Promise((_, reject) => {
		timer = setTimeout(
			() => reject(new Error(`${label} exceeded ${ms}ms (stuck?)`)),
			ms,
		);
		timer.unref?.();
	});
	return Promise.race([promise, guard]).finally(() => clearTimeout(timer));
}

function scenarioKey(s) {
	return [
		s.variantName,
		s.impl,
		s.proto,
		`${s.size}B`,
		`c${s.concurrency}`,
		s.mode,
		s.consume,
		delayMs ? `d${delayMs}ms` : null,
	]
		.filter(Boolean)
		.join(" ");
}

async function runScenario(impl, server, scenario) {
	// Connection: close only exists on h1; multiplexed protocols go cold via
	// a fresh agent per request instead.
	const routeClose =
		scenario.mode === "cold" && ["h1", "h1s"].includes(scenario.proto)
			? "/close"
			: "";
	const routeCache = scenario.route ?? "";
	const routeDelay = delayMs > 0 ? `/delay/${delayMs}` : "";
	const base = new URL(server.url);
	if (scenario.urlHost) base.hostname = scenario.urlHost;
	const url = `${base.origin}${routeClose}${routeCache}${routeDelay}/payload/${scenario.size}`;

	const freshContextPerRequest = scenario.mode === "cold";
	// Warm shares one context; cold builds one per request (below).
	const sharedContext = freshContextPerRequest
		? null
		: impl.makeContext(server, scenario.variant);

	const ttfbs = [];
	const totals = [];
	let failures = 0;
	// A request that neither resolves nor rejects (e.g. a wedged HTTP/2 session
	// after a GOAWAY) would otherwise hang the whole run. Bound every request,
	// and bail out of a scenario that is clearly broken instead of grinding
	// through it. The watchdog is generous so real work (large cold transfers,
	// server `--delay`) never trips it.
	const requestTimeoutMs = Math.max(30_000, delayMs * 3 + 10_000);
	let consecutiveFailures = 0;
	let aborted = false;

	// 1ms resolution: reported delays include the ~1ms timer quantum, which is
	// constant across implementations and so cancels out in comparisons.
	const loopDelay = monitorEventLoopDelay({ resolution: 1 });

	// One request. In cold mode, context construction is part of the measured
	// work — the whole point is "first request on a fresh client" — so it goes
	// inside the timed window; teardown does not. In warm mode the shared
	// context is reused and neither cost is per-request.
	const doRequest = async (record) => {
		let ctx = sharedContext;
		const start = performance.now();
		try {
			if (freshContextPerRequest) {
				ctx = impl.makeContext(server, scenario.variant);
			}
			const res = await withTimeout(
				impl.request(ctx, url, scenario.consume),
				requestTimeoutMs,
				"request",
			);
			const ttfb = performance.now() - start;
			await withTimeout(res.drain(), requestTimeoutMs, "drain");
			const totalMs = performance.now() - start;
			if (res.status !== 200) throw new Error(`status ${res.status}`);
			if (record) {
				ttfbs.push(ttfb);
				totals.push(totalMs);
			}
			consecutiveFailures = 0;
		} catch (err) {
			failures += 1;
			consecutiveFailures += 1;
			if (failures === 1) {
				console.error(`  first failure: ${err.message ?? err}`);
			}
			if (consecutiveFailures >= 15) {
				aborted = true; // scenario is broken; stop rather than hang/grind
			}
		} finally {
			if (freshContextPerRequest && ctx) await impl.closeContext?.(ctx);
		}
	};

	// Closed loop: `concurrency` workers each run requests back to back. Warmup
	// runs first, untimed and unrecorded, so it can't inflate wallMs (and thus
	// deflate rps) or leak into the latency distributions.
	const runPhase = async (count, record) => {
		let issued = 0;
		const worker = async () => {
			while (!aborted) {
				const n = issued++;
				if (n >= count) break;
				await doRequest(record);
			}
		};
		await Promise.all(
			Array.from({ length: scenario.concurrency }, () => worker()),
		);
	};

	await runPhase(warmup, false);

	const wallStart = performance.now();
	loopDelay.enable();
	await runPhase(samples, true);
	loopDelay.disable();
	const wallMs = performance.now() - wallStart;

	if (!freshContextPerRequest) await impl.closeContext?.(sharedContext);

	const record = {
		...scenario,
		variant: undefined, // not serialisable (may contain functions)
		delayMs,
		samples: totals.length,
		failures,
		wallMs,
		rps: (totals.length / wallMs) * 1000,
		ttfb: summarise(ttfbs),
		total: summarise(totals),
		loopDelayP99Ms: loopDelay.percentile(99) / 1e6,
		loopDelayMaxMs: loopDelay.max / 1e6,
	};
	out.write(`${JSON.stringify(record)}\n`);
	return record;
}

function printHeader() {
	console.log(
		`suite=${suite} consume=${consume} samples=${samples} warmup=${warmup}`,
	);
	console.log(
		"scenario".padEnd(44) +
			"ttfb p50/p99".padStart(16) +
			"total p50/p99".padStart(18) +
			"rps".padStart(9) +
			"loop p99".padStart(10),
	);
}

function printResult(scenario, r) {
	console.log(
		scenarioKey(scenario).padEnd(44) +
			`${fmtMs(r.ttfb.p50)}/${fmtMs(r.ttfb.p99)}`.padStart(16) +
			`${fmtMs(r.total.p50)}/${fmtMs(r.total.p99)}`.padStart(18) +
			r.rps.toFixed(0).padStart(9) +
			`${fmtMs(r.loopDelayP99Ms)}ms`.padStart(10) +
			(r.failures ? `  FAILURES=${r.failures}` : ""),
	);
}

async function runMatrix() {
	for (const proto of protos) {
		const server = await startServer(proto);
		try {
			for (const implName of impls) {
				const impl = implementations.get(implName);
				if (!impl) continue; // e.g. node-fetch not installed
				if (!impl.protocols.includes(proto)) continue;
				if (consume === "discard" && implName !== "faith") continue;

				for (const size of sizes) {
					for (const concurrency of concurrencies) {
						for (const mode of modes) {
							const scenario = {
								impl: implName,
								proto,
								size,
								concurrency,
								mode,
								consume,
							};
							printResult(
								scenario,
								await runScenario(impl, server, scenario),
							);
						}
					}
				}
			}
		} finally {
			await server.close();
		}
	}
}

/**
 * Faith-vs-Faith feature comparisons. Variants sharing a `group` are meant to
 * be read against each other; each row differs from its group's baseline by
 * exactly one feature knob.
 */
async function runFeatures() {
	const hasV6 = await ipv6Available();
	const cacheDir = mkdtempSync(path.join(os.tmpdir(), "faith-bench-cache-"));

	const variants = [
		// HTTP versions, same workload
		{ name: "proto:h1", proto: "h1" },
		{ name: "proto:h1s", proto: "h1s" },
		{ name: "proto:h2", proto: "h2" },
		{ name: "proto:h3", proto: "h3" },

		// DNS: hickory (Faith default, with its cache) vs the system resolver.
		// Cold mode so resolution is actually on the measured path; the server
		// is reached via the `localhost` name rather than an IP literal.
		{
			name: "dns:hickory",
			proto: "h1",
			urlHost: "localhost",
			mode: "cold",
		},
		{
			name: "dns:system",
			proto: "h1",
			urlHost: "localhost",
			mode: "cold",
			agentOptions: { dns: { system: true } },
		},

		// Address family (loopback; measures the stack, not routing)
		{ name: "ip:v4", proto: "h1" },
		...(hasV6
			? [{ name: "ip:v6", proto: "h1", serverHost: "::1" }]
			: []),

		// HTTP cache: same cacheable route, no cache vs memory vs disk store.
		// After warmup every cached request is a hit, so these rows measure
		// hit latency against actually fetching from the (local) server.
		{ name: "cache:none", proto: "h1", route: "/cc/3600" },
		{
			name: "cache:memory",
			proto: "h1",
			route: "/cc/3600",
			agentOptions: { cache: { store: "memory" } },
		},
		{
			name: "cache:disk",
			proto: "h1",
			route: "/cc/3600",
			agentOptions: { cache: { store: "disk", path: cacheDir } },
		},

		// Cookie jar off vs on (jar holds one cookie for the origin)
		{ name: "cookies:off", proto: "h1" },
		{
			name: "cookies:on",
			proto: "h1",
			agentOptions: { cookies: true },
			prepare: (agent, server) =>
				agent.addCookie(`${server.url}/`, "bench=1"),
		},
	];

	const impl = implementations.get("faith");
	try {
		for (const variant of variants) {
			const server = await startServer(
				variant.proto,
				variant.serverHost ?? "127.0.0.1",
			);
			try {
				for (const size of sizes) {
					for (const concurrency of concurrencies) {
						const scenario = {
							impl: "faith",
							variantName: variant.name,
							proto: variant.proto,
							size,
							concurrency,
							mode: variant.mode ?? "warm",
							consume,
							route: variant.route,
							urlHost: variant.urlHost,
							variant,
						};
						printResult(
							scenario,
							await runScenario(impl, server, scenario),
						);
					}
				}
			} finally {
				await server.close();
			}
		}
	} finally {
		rmSync(cacheDir, { recursive: true, force: true });
	}
}

printHeader();
if (suite === "features") {
	await runFeatures();
} else {
	await runMatrix();
}

out.end();
console.log(`\nraw samples: ${outPath}`);

// Some clients (node:http2 sessions, got's http2-wrapper cache, libcurl
// handles) keep the event loop alive past the last request; exit cleanly once
// results are flushed.
out.on("finish", () => process.exit(0));
