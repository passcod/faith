#!/usr/bin/env node
/**
 * Render charts from a benchmark results file (NDJSON, as written by run.mjs)
 * using gnuplot. The full and features suites emit far more rows than anyone
 * can read in a table; this turns a run into a handful of SVGs.
 *
 * Usage:
 *   node bench/plot.mjs                     # newest results file, all charts
 *   node bench/plot.mjs --in results/bench-….ndjson
 *   node bench/plot.mjs --out /tmp/plots    # output dir (default results/plots)
 *   node bench/plot.mjs --size 65536 --conc 64 --mode warm   # fix the slice
 *
 * A matrix run (quick/full) produces, one panel per HTTP version:
 *   latency-by-impl   grouped bars, total p50/p99 per implementation
 *   throughput        rps vs concurrency, one line per implementation
 *   latency-vs-size   total p50 vs payload size, one line per implementation
 *   loop-delay        event-loop-delay p99 per implementation
 * A features run produces, one panel per feature group:
 *   features-latency  total p50 per variant
 *   features-rps      throughput per variant
 *
 * gnuplot must be on PATH (e.g. `apt install gnuplot-nox`).
 */
import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const resultsDir = path.join(here, "results");

function flag(name, def = null) {
	const i = process.argv.indexOf(`--${name}`);
	return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : def;
}

// Stable order and a fixed colour per implementation, so a given client is the
// same colour across every chart. faith is a strong red to stand out.
const IMPL_ORDER = [
	"native",
	"undici",
	"http2",
	"got",
	"node-fetch",
	"libcurl",
	"faith",
];
const IMPL_COLOR = {
	native: "#4e79a7",
	undici: "#59a14f",
	http2: "#9c755f",
	got: "#b6992d",
	"node-fetch": "#af7aa1",
	libcurl: "#ff9da7",
	faith: "#e15759",
};
const FALLBACK_COLORS = [
	"#4e79a7", "#f28e2b", "#59a14f", "#e15759", "#b07aa1",
	"#76b7b2", "#edc948", "#ff9da7", "#9c755f", "#bab0ac",
];
const PROTO_ORDER = ["h1", "h1s", "h2", "h3"];
const FEATURE_GROUPS = ["proto", "dns", "ip", "cache", "cookies"];

function uniq(xs) {
	return [...new Set(xs)];
}
function bySpec(order) {
	return (a, b) => {
		const ia = order.indexOf(a);
		const ib = order.indexOf(b);
		return (ia < 0 ? 1e9 : ia) - (ib < 0 ? 1e9 : ib) || String(a).localeCompare(String(b));
	};
}
function gpstr(s) {
	return `"${String(s).replace(/"/g, '\\"')}"`;
}
function humanSize(bytes) {
	if (bytes >= 1024 * 1024) return `${bytes / (1024 * 1024)}MiB`;
	if (bytes >= 1024) return `${bytes / 1024}KiB`;
	return `${bytes}B`;
}

function loadRecords(file) {
	return readFileSync(file, "utf8")
		.split("\n")
		.filter((l) => l.trim())
		.map((l) => JSON.parse(l));
}

function newestResults() {
	const files = readdirSync(resultsDir)
		.filter((f) => f.startsWith("bench-") && f.endsWith(".ndjson"))
		.sort();
	if (!files.length) {
		throw new Error(`no bench-*.ndjson files in ${resultsDir}`);
	}
	return path.join(resultsDir, files.at(-1));
}

/** Run one gnuplot script (string) producing an SVG at outPath. */
function gnuplot(script, outPath) {
	const gp = `${outPath}.gp`;
	writeFileSync(gp, script);
	execFileSync("gnuplot", [gp], { stdio: ["ignore", "ignore", "inherit"] });
	return outPath;
}

// Output format: svg (default, scalable — good for READMEs) or png.
const FORMAT = flag("format", "svg") === "png" ? "png" : "svg";
const TERMINAL =
	FORMAT === "png"
		? (w, h) => `set terminal pngcairo size ${w},${h} font "sans,12"`
		: (w, h) => `set terminal svg size ${w},${h} font "sans,12" background rgb "white"`;

const SVG_HEAD = (w, h, outPath) =>
	`${TERMINAL(w, h)}\nset output ${gpstr(outPath)}\n`;

/**
 * Multiplot of grouped-bar panels. `panels` is [{title, categories, series}]
 * where series is [{label, color, values: number[] aligned to categories}].
 */
function barMultiplot({ outPath, suptitle, ylabel, panels, cols }) {
	const rows = Math.ceil(panels.length / cols);
	let s = SVG_HEAD(480 * cols, 340 * rows, outPath);
	s += `set multiplot layout ${rows},${cols} title ${gpstr(suptitle)} font "sans,15"\n`;
	s += `set style data histograms\nset style histogram clustered gap 1\n`;
	s += `set style fill solid 0.85 border -1\nset boxwidth 0.9\n`;
	s += `set grid ytics\nset yrange [0:*]\nset key outside top right font "sans,9"\n`;
	s += `set xtics rotate by -35 font "sans,9"\n`;
	panels.forEach((p, pi) => {
		const dn = `$bar${pi}`;
		s += `${dn} << EOD\n`;
		p.categories.forEach((cat, ci) => {
			s += `${gpstr(cat)} ${p.series.map((se) => (Number.isFinite(se.values[ci]) ? se.values[ci] : "NaN")).join(" ")}\n`;
		});
		s += `EOD\n`;
		s += `set title ${gpstr(p.title)} font "sans,12"\n`;
		s += `set ylabel ${gpstr(ylabel)} font "sans,10"\n`;
		const plots = p.series
			.map(
				(se, si) =>
					`${si === 0 ? dn : "''"} using ${si + 2}${si === 0 ? ":xtic(1)" : ""} title ${gpstr(se.label)} lc rgb ${gpstr(se.color)}`,
			)
			.join(", ");
		s += `plot ${plots}\n`;
	});
	s += `unset multiplot\nunset output\n`;
	return gnuplot(s, outPath);
}

/**
 * Multiplot of line panels. `panels` is [{title, xlabel, xlog, lines}] where
 * lines is [{label, color, points: [x,y][]}].
 */
function lineMultiplot({ outPath, suptitle, ylabel, panels, cols }) {
	const rows = Math.ceil(panels.length / cols);
	let s = SVG_HEAD(480 * cols, 340 * rows, outPath);
	s += `set multiplot layout ${rows},${cols} title ${gpstr(suptitle)} font "sans,15"\n`;
	s += `set grid xtics ytics\nset yrange [0:*]\nset key outside top right font "sans,9"\n`;
	panels.forEach((p, pi) => {
		p.lines.forEach((ln, li) => {
			const dn = `$ln${pi}_${li}`;
			s += `${dn} << EOD\n`;
			ln.points.forEach(([x, y]) => {
				s += `${x} ${Number.isFinite(y) ? y : "NaN"}\n`;
			});
			s += `EOD\n`;
		});
		s += p.xlog ? `set logscale x 2\n` : `unset logscale x\n`;
		if (p.xtics) {
			s += `set xtics (${p.xtics.map(([pos, lab]) => `${gpstr(lab)} ${pos}`).join(", ")}) font "sans,9"\n`;
		} else {
			s += `set xtics auto font "sans,9"\n`;
		}
		s += `set title ${gpstr(p.title)} font "sans,12"\n`;
		s += `set xlabel ${gpstr(p.xlabel)} font "sans,10"\nset ylabel ${gpstr(ylabel)} font "sans,10"\n`;
		const plots = p.lines
			.map(
				(ln, li) =>
					`$ln${pi}_${li} using 1:2 with linespoints pt 7 ps 0.7 lw 2 lc rgb ${gpstr(ln.color)} title ${gpstr(ln.label)}`,
			)
			.join(", ");
		s += `plot ${plots}\n`;
	});
	s += `unset multiplot\nunset output\n`;
	return gnuplot(s, outPath);
}

function implColor(impl, i) {
	return IMPL_COLOR[impl] ?? FALLBACK_COLORS[i % FALLBACK_COLORS.length];
}

// ---------------------------------------------------------------- matrix ----

function plotMatrix(records, outDir) {
	const protos = uniq(records.map((r) => r.proto)).sort(bySpec(PROTO_ORDER));
	const impls = uniq(records.map((r) => r.impl)).sort(bySpec(IMPL_ORDER));
	const sizes = uniq(records.map((r) => r.size)).sort((a, b) => a - b);
	const concs = uniq(records.map((r) => r.concurrency)).sort((a, b) => a - b);
	const modes = uniq(records.map((r) => r.mode));

	const mode = flag("mode", modes.includes("warm") ? "warm" : modes[0]);
	const size = Number(
		flag("size", sizes.includes(65536) ? 65536 : sizes[Math.floor(sizes.length / 2)]),
	);
	const conc = Number(flag("conc", concs.at(-1)));
	const cols = protos.length <= 1 ? 1 : 2;
	const written = [];

	const at = (pred) => records.filter(pred);
	const one = (rs) => (rs.length ? rs[0] : null);

	// 1. latency by impl: grouped p50/p99 bars, per proto, at (size, conc, mode)
	{
		const panels = protos.map((proto) => {
			const cats = impls.filter((im) =>
				at((r) => r.proto === proto && r.impl === im && r.size === size && r.concurrency === conc && r.mode === mode).length,
			);
			const pick = (im, k) => {
				const r = one(at((x) => x.proto === proto && x.impl === im && x.size === size && x.concurrency === conc && x.mode === mode));
				return r ? r.total[k] : NaN;
			};
			return {
				title: `${proto} — ${humanSize(size)}, c${conc}, ${mode}`,
				categories: cats,
				series: [
					{ label: "p50", color: "#4e79a7", values: cats.map((im) => pick(im, "p50")) },
					{ label: "p99", color: "#e15759", values: cats.map((im) => pick(im, "p99")) },
				],
			};
		}).filter((p) => p.categories.length);
		if (panels.length) {
			written.push(barMultiplot({
				outPath: path.join(outDir, `latency-by-impl.${FORMAT}`),
				suptitle: `Response latency by implementation (ms, lower is better)`,
				ylabel: "ms",
				panels, cols,
			}));
		}
	}

	// 2. throughput vs concurrency: line per impl, per proto, at (size, mode)
	if (concs.length > 1) {
		const panels = protos.map((proto) => {
			const lines = impls.map((im, i) => ({
				label: im, color: implColor(im, i),
				points: concs.map((c) => {
					const r = one(at((x) => x.proto === proto && x.impl === im && x.size === size && x.concurrency === c && x.mode === mode));
					return [c, r ? r.rps : NaN];
				}).filter(([, y]) => Number.isFinite(y)),
			})).filter((ln) => ln.points.length);
			return { title: `${proto} — ${humanSize(size)}, ${mode}`, xlabel: "concurrency", xlog: false, lines };
		}).filter((p) => p.lines.length);
		if (panels.length) {
			written.push(lineMultiplot({
				outPath: path.join(outDir, `throughput.${FORMAT}`),
				suptitle: `Throughput vs concurrency (requests/s, higher is better)`,
				ylabel: "req/s", panels, cols,
			}));
		}
	}

	// 3. latency vs payload size: line per impl, per proto, at (conc, mode)
	if (sizes.length > 1) {
		const panels = protos.map((proto) => {
			const lines = impls.map((im, i) => ({
				label: im, color: implColor(im, i),
				points: sizes.map((sz) => {
					const r = one(at((x) => x.proto === proto && x.impl === im && x.size === sz && x.concurrency === conc && x.mode === mode));
					return [Math.max(sz, 1), r ? r.total.p50 : NaN];
				}).filter(([, y]) => Number.isFinite(y)),
			})).filter((ln) => ln.points.length);
			return { title: `${proto} — c${conc}, ${mode}`, xlabel: "payload size (log)", xlog: true, lines,
					xtics: sizes.map((sz) => [Math.max(sz, 1), humanSize(sz)]) };
		}).filter((p) => p.lines.length);
		if (panels.length) {
			written.push(lineMultiplot({
				outPath: path.join(outDir, `latency-vs-size.${FORMAT}`),
				suptitle: `Latency vs payload size (total p50 ms, lower is better)`,
				ylabel: "ms", panels, cols,
			}));
		}
	}

	// 4. event-loop delay p99: bar per impl, per proto, at (size, conc, mode)
	{
		const panels = protos.map((proto) => {
			const cats = impls.filter((im) =>
				at((r) => r.proto === proto && r.impl === im && r.size === size && r.concurrency === conc && r.mode === mode).length,
			);
			const pick = (im) => {
				const r = one(at((x) => x.proto === proto && x.impl === im && x.size === size && x.concurrency === conc && x.mode === mode));
				return r ? r.loopDelayP99Ms : NaN;
			};
			return {
				title: `${proto} — ${humanSize(size)}, c${conc}, ${mode}`,
				categories: cats,
				series: [{ label: "loop delay p99", color: "#59a14f", values: cats.map(pick) }],
			};
		}).filter((p) => p.categories.length);
		if (panels.length) {
			written.push(barMultiplot({
				outPath: path.join(outDir, `loop-delay.${FORMAT}`),
				suptitle: `Event-loop delay p99 (ms, lower is better — JS blocked while moving bytes)`,
				ylabel: "ms", panels, cols,
			}));
		}
	}

	return { written, note: `matrix slice: size=${humanSize(size)} conc=${conc} mode=${mode} (override with --size/--conc/--mode)` };
}

// -------------------------------------------------------------- features ----

function plotFeatures(records, outDir) {
	const sizes = uniq(records.map((r) => r.size)).sort((a, b) => a - b);
	const concs = uniq(records.map((r) => r.concurrency)).sort((a, b) => a - b);
	const size = Number(flag("size", sizes.includes(1024) ? 1024 : sizes[0]));
	const conc = Number(flag("conc", concs.at(-1)));
	const rs = records.filter((r) => r.size === size && r.concurrency === conc);

	const groupsPresent = FEATURE_GROUPS.filter((g) =>
		rs.some((r) => (r.variantName ?? "").startsWith(`${g}:`)),
	);
	const cols = groupsPresent.length <= 1 ? 1 : Math.min(3, groupsPresent.length);
	const written = [];

	const build = (outName, suptitle, ylabel, metric) => {
		const panels = groupsPresent.map((g) => {
			const variants = rs
				.filter((r) => (r.variantName ?? "").startsWith(`${g}:`))
				.sort(bySpec([]));
			const cats = variants.map((r) => r.variantName.slice(g.length + 1));
			return {
				title: g,
				categories: cats,
				series: [{ label: ylabel, color: "#e15759", values: variants.map(metric) }],
			};
		}).filter((p) => p.categories.length);
		if (panels.length) {
			written.push(barMultiplot({
				outPath: path.join(outDir, `${outName}.${FORMAT}`),
				suptitle, ylabel, panels, cols,
			}));
		}
	};

	build("features-latency", `fáith feature comparison — total p50 (ms) @ ${humanSize(size)}, c${conc}`, "ms", (r) => r.total.p50);
	build("features-rps", `fáith feature comparison — throughput (req/s) @ ${humanSize(size)}, c${conc}`, "req/s", (r) => r.rps);

	return { written, note: `features slice: size=${humanSize(size)} conc=${conc} (override with --size/--conc)` };
}

// ------------------------------------------------------------------ main ----

const inFile = flag("in") ? path.resolve(flag("in")) : newestResults();
const outDir = flag("out", path.join(resultsDir, "plots"));
mkdirSync(outDir, { recursive: true });

const records = loadRecords(inFile);
if (!records.length) {
	console.error(`no records in ${inFile}`);
	process.exit(1);
}
const isFeatures = records.some((r) => r.variantName);

console.log(`input:  ${path.relative(process.cwd(), inFile)} (${records.length} records, ${isFeatures ? "features" : "matrix"} suite)`);
const { written, note } = isFeatures ? plotFeatures(records, outDir) : plotMatrix(records, outDir);
console.log(note);
if (!written.length) {
	console.error("no charts produced (empty slice?) — try different --size/--conc");
	process.exit(1);
}
for (const f of written) console.log(`  ${path.relative(process.cwd(), f)}`);
