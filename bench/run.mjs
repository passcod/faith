#!/usr/bin/env node
/**
 * fáith benchmark runner.
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
 *   node bench/run.mjs [--suite quick|full]
 *     [--impls native,faith,node-fetch] [--protos h1,h1s,h2]
 *     [--sizes 0,1024,65536,1048576] [--conc 1,16,64] [--modes warm,cold]
 *     [--consume bytes|text|discard] [--samples 200] [--warmup 50]
 *     [--delay 0] [--out bench/results]
 */

import { spawnSync } from "node:child_process";
import { mkdirSync, createWriteStream } from "node:fs";
import path from "node:path";
import { monitorEventLoopDelay, performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

import { loadImpls } from "./lib/clients.mjs";
import { ensureCert, startServer } from "./lib/servers.mjs";
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
				impls: ["native", "faith", "node-fetch"],
				protos: ["h1", "h1s", "h2"],
				sizes: [0, 1024, 65536, 1048576],
				conc: [1, 16, 64],
				modes: ["warm", "cold"],
				samples: 500,
				warmup: 100,
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
	protos.some((p) => p !== "h1") &&
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

function scenarioKey(s) {
	return [
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
	const routeBase = scenario.mode === "cold" && scenario.proto !== "h2"
		? "/close"
		: "";
	const routeDelay = delayMs > 0 ? `/delay/${delayMs}` : "";
	const url = `${server.url}${routeBase}${routeDelay}/payload/${scenario.size}`;

	const freshContextPerRequest = scenario.mode === "cold";
	let context = impl.makeContext();

	const ttfbs = [];
	const totals = [];
	const total = warmup + samples;
	let issued = 0;
	let failures = 0;

	// 1ms resolution: reported delays include the ~1ms timer quantum, which is
	// constant across implementations and so cancels out in comparisons.
	const loopDelay = monitorEventLoopDelay({ resolution: 1 });

	const worker = async () => {
		while (issued < total) {
			const n = issued++;
			const ctx = freshContextPerRequest ? impl.makeContext() : context;
			const start = performance.now();
			try {
				const res = await impl.request(ctx, url, scenario.consume);
				const ttfb = performance.now() - start;
				await res.drain();
				const totalMs = performance.now() - start;
				if (res.status !== 200) throw new Error(`status ${res.status}`);
				if (n >= warmup) {
					ttfbs.push(ttfb);
					totals.push(totalMs);
				}
			} catch (err) {
				failures += 1;
				if (failures === 1) {
					console.error(`  first failure: ${err.message ?? err}`);
				}
			}
		}
	};

	// Closed loop: `concurrency` workers each run requests back to back.
	const wallStart = performance.now();
	loopDelay.enable();
	await Promise.all(
		Array.from({ length: scenario.concurrency }, () => worker()),
	);
	loopDelay.disable();
	const wallMs = performance.now() - wallStart;

	const record = {
		...scenario,
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

console.log(`suite=${suite} consume=${consume} samples=${samples} warmup=${warmup}`);
console.log(
	"scenario".padEnd(38) +
		"ttfb p50/p99".padStart(16) +
		"total p50/p99".padStart(18) +
		"rps".padStart(9) +
		"loop p99".padStart(10),
);

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
						const r = await runScenario(impl, server, scenario);
						const line =
							scenarioKey(scenario).padEnd(38) +
							`${fmtMs(r.ttfb.p50)}/${fmtMs(r.ttfb.p99)}`.padStart(16) +
							`${fmtMs(r.total.p50)}/${fmtMs(r.total.p99)}`.padStart(18) +
							r.rps.toFixed(0).padStart(9) +
							`${fmtMs(r.loopDelayP99Ms)}ms`.padStart(10) +
							(r.failures ? `  FAILURES=${r.failures}` : "");
						console.log(line);
					}
				}
			}
		}
	} finally {
		await server.close();
	}
}

out.end();
console.log(`\nraw samples: ${outPath}`);
