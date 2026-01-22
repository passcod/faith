#!/usr/bin/env node

import { readFile, writeFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { cpus } from "node:os";

const execFileAsync = promisify(execFile);

const OUTPUT_DIR = "charts";
const DATA_FILE = "bench-data.json";

// Helper to run commands in parallel
async function parallel(items, fn, concurrency = cpus().length) {
	const results = [];
	const queue = [...items];
	const workers = [];

	for (let i = 0; i < concurrency; i++) {
		workers.push(
			(async () => {
				while (queue.length > 0) {
					const item = queue.shift();
					if (item !== undefined) {
						results.push(await fn(item));
					}
				}
			})(),
		);
	}

	await Promise.all(workers);
	return results;
}

// Extract packet stats from a pcap file
async function extractPcapStats(filename) {
	const pcapFile = `data/${filename}.pcap`;

	if (!existsSync(pcapFile)) {
		return {
			filename,
			total: 0,
			tcp: 0,
			udp: 0,
			bytes: 0,
			connections: 0,
			dnsQueries: 0,
			dnsResponses: 0,
		};
	}

	try {
		const [
			totalResult,
			tcpResult,
			udpResult,
			bytesResult,
			tcpSynResult,
			quicInitialResult,
			dnsQueryResult,
			dnsResponseResult,
		] = await Promise.all([
			execFileAsync("tcpdump", ["-r", pcapFile, "--count"], {
				encoding: "utf8",
			}).catch(() => ({ stdout: "0 packets" })),
			execFileAsync("tcpdump", ["-r", pcapFile, "--count", "tcp"], {
				encoding: "utf8",
			}).catch(() => ({ stdout: "0 packets" })),
			execFileAsync("tcpdump", ["-r", pcapFile, "--count", "udp"], {
				encoding: "utf8",
			}).catch(() => ({ stdout: "0 packets" })),
			execFileAsync("capinfos", ["-M", pcapFile], {
				encoding: "utf8",
			}).catch(() => ({ stdout: "" })),
			execFileAsync(
				"tcpdump",
				[
					"-r",
					pcapFile,
					"--count",
					"tcp[tcpflags] & tcp-syn != 0 and tcp[tcpflags] & tcp-ack == 0",
				],
				{
					encoding: "utf8",
				},
			).catch(() => ({ stdout: "0 packets" })),
			// Try tshark for proper QUIC Initial packet detection, fall back to heuristic
			execFileAsync(
				"tshark",
				[
					"-r",
					pcapFile,
					"-Y",
					"quic.long.packet_type == 0",
					"-T",
					"fields",
					"-e",
					"frame.number",
				],
				{
					encoding: "utf8",
				},
			)
				.then((result) => ({
					stdout:
						result.stdout.trim().split("\n").length + " packets",
				}))
				.catch(() =>
					// Fall back to counting large UDP packets as a heuristic
					execFileAsync(
						"tcpdump",
						[
							"-r",
							pcapFile,
							"--count",
							"-n",
							"udp and greater 1200",
						],
						{
							encoding: "utf8",
						},
					).catch(() => ({ stdout: "0 packets" })),
				),
			// Use tshark to detect DNS queries specifically
			execFileAsync(
				"tshark",
				[
					"-r",
					pcapFile,
					"-Y",
					"dns.flags.response == 0",
					"-T",
					"fields",
					"-e",
					"frame.number",
				],
				{
					encoding: "utf8",
				},
			)
				.then((result) => {
					const lines = result.stdout.trim();
					return {
						stdout: lines
							? lines.split("\n").length + " packets"
							: "0 packets",
					};
				})
				.catch(() =>
					// Fall back to tcpdump if tshark fails
					execFileAsync(
						"tcpdump",
						["-r", pcapFile, "--count", "udp port 53"],
						{
							encoding: "utf8",
						},
					).catch(() => ({ stdout: "0 packets" })),
				),
			// Use tshark to detect DNS responses specifically
			execFileAsync(
				"tshark",
				[
					"-r",
					pcapFile,
					"-Y",
					"dns.flags.response == 1",
					"-T",
					"fields",
					"-e",
					"frame.number",
				],
				{
					encoding: "utf8",
				},
			)
				.then((result) => {
					const lines = result.stdout.trim();
					return {
						stdout: lines
							? lines.split("\n").length + " packets"
							: "0 packets",
					};
				})
				.catch(() =>
					// Fall back to tcpdump if tshark fails
					execFileAsync(
						"tcpdump",
						["-r", pcapFile, "--count", "udp port 53"],
						{
							encoding: "utf8",
						},
					).catch(() => ({ stdout: "0 packets" })),
				),
		]);

		const total = parseInt(totalResult.stdout.split(" ")[0]) || 0;
		const tcp = parseInt(tcpResult.stdout.split(" ")[0]) || 0;
		const udp = parseInt(udpResult.stdout.split(" ")[0]) || 0;

		const bytesMatch = bytesResult.stdout.match(/^Data size:\s+(\d+)/m);
		const bytes = bytesMatch ? parseInt(bytesMatch[1]) : 0;

		const tcpSyn = parseInt(tcpSynResult.stdout.split(" ")[0]) || 0;
		const quicInitialCount =
			parseInt(quicInitialResult.stdout.split(" ")[0]) || 0;

		// For QUIC: if tshark worked, we have accurate Initial packet count
		// Otherwise quicInitialCount is from the tcpdump heuristic (large packets)
		// Heuristic: estimate ~1 connection per 20 large UDP packets
		const quicConnections =
			udp > 0 && quicInitialCount > 10
				? Math.max(1, Math.round(quicInitialCount / 20))
				: quicInitialCount;

		const connections = tcp > 0 ? tcpSyn : udp > 0 ? quicConnections : 0;

		const dnsQueries = parseInt(dnsQueryResult.stdout.split(" ")[0]) || 0;
		const dnsResponses =
			parseInt(dnsResponseResult.stdout.split(" ")[0]) || 0;

		return {
			filename,
			total,
			tcp,
			udp,
			bytes,
			connections,
			dnsQueries,
			dnsResponses,
		};
	} catch (err) {
		console.error(`Error reading ${pcapFile}:`, err.message);
		return {
			filename,
			total: 0,
			tcp: 0,
			udp: 0,
			bytes: 0,
			connections: 0,
			dnsQueries: 0,
			dnsResponses: 0,
		};
	}
}

// Calculate statistics for a group of durations
function calculateStats(durations) {
	if (durations.length === 0)
		return { mean: 0, median: 0, min: 0, max: 0, stddev: 0 };

	const sorted = [...durations].sort((a, b) => a - b);
	const sum = sorted.reduce((a, b) => a + b, 0);
	const mean = sum / sorted.length;

	const median =
		sorted.length % 2 === 0
			? (sorted[sorted.length / 2 - 1] + sorted[sorted.length / 2]) / 2
			: sorted[Math.floor(sorted.length / 2)];

	const variance =
		sorted.reduce((sum, val) => sum + Math.pow(val - mean, 2), 0) /
		sorted.length;
	const stddev = Math.sqrt(variance);

	return {
		mean,
		median,
		min: sorted[0],
		max: sorted[sorted.length - 1],
		stddev,
	};
}

// Remove outliers using IQR method
function removeOutliers(durations) {
	if (durations.length < 4) return durations;

	const sorted = [...durations].sort((a, b) => a - b);
	const q1Index = Math.floor(sorted.length * 0.25);
	const q3Index = Math.floor(sorted.length * 0.75);
	const q1 = sorted[q1Index];
	const q3 = sorted[q3Index];
	const iqr = q3 - q1;
	const lowerBound = q1 - 1.5 * iqr;
	const upperBound = q3 + 1.5 * iqr;

	return durations.filter((d) => d >= lowerBound && d <= upperBound);
}

// Generate a gnuplot script and data file
async function generateChart(name, config) {
	const output = `${name}.png`;
	const dataFile = `${name}_data.txt`;
	const dataPath = `charts/${dataFile}`;

	const gnuplotScript = `set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/${output}'
set title '${config.title}'
set xlabel '${config.xlabel}'
set ylabel '${config.ylabel}'
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
${config.xtics || 'set xtics ("1" 0, "10" 1, "100" 2)'}
${config.extra || ""}

${typeof config.plot === "function" ? config.plot(dataPath) : config.plot}
`;

	await writeFile(`${OUTPUT_DIR}/${name}.gnuplot`, gnuplotScript);
	await writeFile(`${OUTPUT_DIR}/${dataFile}`, config.data);

	try {
		await execFileAsync("gnuplot", [`${OUTPUT_DIR}/${name}.gnuplot`]);
		console.log(`✓ Generated: ${output}`);
	} catch (err) {
		console.error(`✗ Failed to generate ${output}:`, err.message);
	}

	// Generate zero-baseline variant
	const zeroOutput = `${name}_zero.png`;
	const gnuplotScriptZero = `set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/${zeroOutput}'
set title '${config.title} (Y-axis from zero)'
set xlabel '${config.xlabel}'
set ylabel '${config.ylabel}'
set yrange [0:*]
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
${config.xtics || 'set xtics ("1" 0, "10" 1, "100" 2)'}
${config.extra || ""}

${typeof config.plot === "function" ? config.plot(dataPath) : config.plot}
`;

	await writeFile(`${OUTPUT_DIR}/${name}_zero.gnuplot`, gnuplotScriptZero);

	try {
		await execFileAsync("gnuplot", [`${OUTPUT_DIR}/${name}_zero.gnuplot`]);
		console.log(`✓ Generated: ${zeroOutput}`);
	} catch (err) {
		console.error(`✗ Failed to generate ${zeroOutput}:`, err.message);
	}
}

async function main() {
	console.log("Reading benchmark data...");

	if (!existsSync(DATA_FILE)) {
		console.error(`Error: ${DATA_FILE} not found`);
		process.exit(1);
	}

	const rawData = await readFile(DATA_FILE, "utf8");
	const benchData = JSON.parse(rawData);
	const entries = Object.values(benchData);

	await mkdir(OUTPUT_DIR, { recursive: true });

	// Extract packet statistics in parallel
	console.log("Extracting packet counts from pcap files...");
	const filenames = entries.map((e) => e.filename);
	const packetStats = await parallel(filenames, extractPcapStats);
	const packetStatsMap = new Map(packetStats.map((s) => [s.filename, s]));

	// Helper to group and aggregate data
	function groupBy(entries, keyFn, valueFn) {
		const groups = new Map();
		for (const entry of entries) {
			const key = keyFn(entry);
			if (!groups.has(key)) groups.set(key, []);
			groups.get(key).push(valueFn ? valueFn(entry) : entry);
		}
		return groups;
	}

	// Helper to get average duration for a filter (with outlier removal)
	function getAvgDuration(filter) {
		const filtered = entries.filter(filter);
		if (filtered.length === 0) return 0;
		const durations = filtered.map((e) => e.duration);
		const cleaned = removeOutliers(durations);
		if (cleaned.length === 0) return 0;
		return cleaned.reduce((sum, d) => sum + d, 0) / cleaned.length;
	}

	// Helper to get overhead (x10/x100 minus x1 baseline)
	function calculateOverhead(impl, target, http3, hits) {
		const x1 = getAvgDuration(
			(e) =>
				e.impl === impl &&
				e.target === target &&
				e.http3 === http3 &&
				e.hits === 1,
		);
		const xN = getAvgDuration(
			(e) =>
				e.impl === impl &&
				e.target === target &&
				e.http3 === http3 &&
				e.hits === hits,
		);
		return x1 > 0 ? xN - x1 : 0;
	}

	console.log("Generating performance comparison charts...");

	// Local performance
	const localData = [1, 10, 100]
		.map((hits) => {
			const native = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === hits,
			);
			const nodefetch = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === hits,
			);
			const faith = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === hits,
			);
			return `${hits}\t${native}\t${nodefetch}\t${faith}`;
		})
		.join("\n");

	await generateChart("performance_local", {
		title: "Performance Comparison (Local Target)",
		xlabel: "Number of Requests",
		ylabel: "Duration (ms)",
		data: localData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Local overhead
	const localOverheadData = [10, 100]
		.map((hits) => {
			const native = calculateOverhead("native", "local", false, hits);
			const nodefetch = calculateOverhead(
				"node-fetch",
				"local",
				false,
				hits,
			);
			const faith = calculateOverhead("faith", "local", false, hits);
			return `${hits}\t${native}\t${nodefetch}\t${faith}`;
		})
		.join("\n");

	await generateChart("performance_overhead_local", {
		title: "Request Overhead (Local Target - minus x1 baseline)",
		xlabel: "Number of Requests",
		ylabel: "Overhead Duration (ms)",
		data: localOverheadData,
		xtics: 'set xtics ("10" 0, "100" 1)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Parallelism impact (native-local, HITS=100, varying SEQ)
	console.log("Generating parallelism comparison charts...");

	const parallelismNativeLocalData = [1, 10, 25, 50]
		.map((seq) => {
			const duration = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			return `${seq}\t${duration}`;
		})
		.join("\n");

	await generateChart("parallelism_native_local", {
		title: "Parallelism Impact: native (Local Target, 100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Duration (ms)",
		data: parallelismNativeLocalData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native' with boxes, \\
     '' using 0:2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Throughput by parallelism (native-local, HITS=100, varying SEQ)
	const throughputParallelismData = [1, 10, 25, 50]
		.map((seq) => {
			const duration = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const throughput =
				duration > 0 ? (100 / (duration / 1000)).toFixed(1) : 0;
			return `${seq}\t${throughput}`;
		})
		.join("\n");

	await generateChart("throughput_parallelism_native_local", {
		title: "Throughput by Parallelism: native (Local Target, 100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Requests/Second",
		data: throughputParallelismData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native' with boxes, \\
     '' using 0:2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Parallelism impact (local, HITS=100, varying SEQ)
	const parallelismLocalData = [1, 10, 25, 50]
		.map((seq) => {
			const native = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nodefetch = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faith = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			return `${seq}\t${native}\t${nodefetch}\t${faith}`;
		})
		.join("\n");

	await generateChart("parallelism_local", {
		title: "Parallelism Impact: Local Target (100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Duration (ms)",
		data: parallelismLocalData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Throughput by parallelism (local, HITS=100, varying SEQ)
	const throughputParallelismLocalData = [1, 10, 25, 50]
		.map((seq) => {
			const nativeDuration = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nodefetchDuration = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithDuration = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "local" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nativeThroughput =
				nativeDuration > 0
					? (100 / (nativeDuration / 1000)).toFixed(1)
					: 0;
			const nodefetchThroughput =
				nodefetchDuration > 0
					? (100 / (nodefetchDuration / 1000)).toFixed(1)
					: 0;
			const faithThroughput =
				faithDuration > 0
					? (100 / (faithDuration / 1000)).toFixed(1)
					: 0;
			return `${seq}\t${nativeThroughput}\t${nodefetchThroughput}\t${faithThroughput}`;
		})
		.join("\n");

	await generateChart("throughput_parallelism_local", {
		title: "Throughput by Parallelism: Local Target (100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Requests/Second",
		data: throughputParallelismLocalData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Parallelism impact (google, HITS=100, varying SEQ)
	const parallelismGoogleData = [1, 10, 25, 50]
		.map((seq) => {
			const native = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nodefetch = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithTcp = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithQuicCubic = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === "cubic" &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithQuicBbr = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === "bbr" &&
					e.hits === 100 &&
					e.seq === seq,
			);
			return `${seq}\t${native}\t${nodefetch}\t${faithTcp}\t${faithQuicCubic}\t${faithQuicBbr}`;
		})
		.join("\n");

	await generateChart("parallelism_google", {
		title: "Parallelism Impact: Google Target (100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Duration (ms)",
		data: parallelismGoogleData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC-Cubic', \\
     '' using 6 title 'Fáith-QUIC-BBR', \\
     '' using ($0-0.4):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.2):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.2):5:(sprintf("%.0f",$5)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.4):6:(sprintf("%.0f",$6)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Throughput by parallelism (google, HITS=100, varying SEQ)
	const throughputParallelismGoogleData = [1, 10, 25, 50]
		.map((seq) => {
			const nativeDuration = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nodefetchDuration = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithTcpDuration = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithQuicCubicDuration = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === "cubic" &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithQuicBbrDuration = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === "bbr" &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nativeThroughput =
				nativeDuration > 0
					? (100 / (nativeDuration / 1000)).toFixed(1)
					: 0;
			const nodefetchThroughput =
				nodefetchDuration > 0
					? (100 / (nodefetchDuration / 1000)).toFixed(1)
					: 0;
			const faithTcpThroughput =
				faithTcpDuration > 0
					? (100 / (faithTcpDuration / 1000)).toFixed(1)
					: 0;
			const faithQuicCubicThroughput =
				faithQuicCubicDuration > 0
					? (100 / (faithQuicCubicDuration / 1000)).toFixed(1)
					: 0;
			const faithQuicBbrThroughput =
				faithQuicBbrDuration > 0
					? (100 / (faithQuicBbrDuration / 1000)).toFixed(1)
					: 0;
			return `${seq}\t${nativeThroughput}\t${nodefetchThroughput}\t${faithTcpThroughput}\t${faithQuicCubicThroughput}\t${faithQuicBbrThroughput}`;
		})
		.join("\n");

	await generateChart("throughput_parallelism_google", {
		title: "Throughput by Parallelism: Google Target (100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Requests/Second",
		data: throughputParallelismGoogleData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC-Cubic', \\
     '' using 6 title 'Fáith-QUIC-BBR', \\
     '' using ($0-0.4):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.2):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.2):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.4):6:(sprintf("%.1f",$6)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Parallelism impact (cloudflare, HITS=100, varying SEQ)
	const parallelismCloudflareData = [1, 10, 25, 50]
		.map((seq) => {
			const native = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "cloudflare-100k" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nodefetch = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "cloudflare-100k" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faith = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "cloudflare-100k" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			return `${seq}\t${native}\t${nodefetch}\t${faith}`;
		})
		.join("\n");

	await generateChart("parallelism_cloudflare", {
		title: "Parallelism Impact: Cloudflare Target (100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Duration (ms)",
		data: parallelismCloudflareData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Throughput by parallelism (cloudflare, HITS=100, varying SEQ)
	const throughputParallelismCloudflareData = [1, 10, 25, 50]
		.map((seq) => {
			const nativeDuration = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "cloudflare-100k" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nodefetchDuration = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "cloudflare-100k" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const faithDuration = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "cloudflare-100k" &&
					e.http3 === false &&
					e.hits === 100 &&
					e.seq === seq,
			);
			const nativeThroughput =
				nativeDuration > 0
					? (100 / (nativeDuration / 1000)).toFixed(1)
					: 0;
			const nodefetchThroughput =
				nodefetchDuration > 0
					? (100 / (nodefetchDuration / 1000)).toFixed(1)
					: 0;
			const faithThroughput =
				faithDuration > 0
					? (100 / (faithDuration / 1000)).toFixed(1)
					: 0;
			return `${seq}\t${nativeThroughput}\t${nodefetchThroughput}\t${faithThroughput}`;
		})
		.join("\n");

	await generateChart("throughput_parallelism_cloudflare", {
		title: "Throughput by Parallelism: Cloudflare Target (100 requests)",
		xlabel: "Parallel Requests (SEQ)",
		ylabel: "Requests/Second",
		data: throughputParallelismCloudflareData,
		xtics: 'set xtics ("1" 0, "10" 1, "25" 2, "50" 3)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Google performance
	const googleData = [1, 10, 100]
		.map((hits) => {
			const native = getAvgDuration(
				(e) =>
					e.impl === "native" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === hits,
			);
			const nodefetch = getAvgDuration(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === hits,
			);
			const faith = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === hits,
			);
			return `${hits}\t${native}\t${nodefetch}\t${faith}`;
		})
		.join("\n");

	await generateChart("performance_google", {
		title: "Performance Comparison (Google Target - TCP)",
		xlabel: "Number of Requests",
		ylabel: "Duration (ms)",
		data: googleData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Google overhead
	const googleOverheadData = [10, 100]
		.map((hits) => {
			const native = calculateOverhead("native", "google", false, hits);
			const nodefetch = calculateOverhead(
				"node-fetch",
				"google",
				false,
				hits,
			);
			const faith = calculateOverhead("faith", "google", false, hits);
			return `${hits}\t${native}\t${nodefetch}\t${faith}`;
		})
		.join("\n");

	await generateChart("performance_overhead_google", {
		title: "Request Overhead (Google Target - TCP, minus x1 baseline)",
		xlabel: "Number of Requests",
		ylabel: "Overhead Duration (ms)",
		data: googleOverheadData,
		xtics: 'set xtics ("10" 0, "100" 1)',
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Protocol comparison
	const protocolData = [1, 10, 100]
		.map((hits) => {
			const tcp = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === false &&
					e.hits === hits,
			);
			const cubic = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === "cubic" &&
					e.hits === hits,
			);
			const bbr = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.http3 === "bbr" &&
					e.hits === hits,
			);
			return `${hits}\t${tcp}\t${cubic}\t${bbr}`;
		})
		.join("\n");

	await generateChart("protocol_comparison", {
		title: "Fáith: TCP vs QUIC (Google Target)",
		xlabel: "Number of Requests",
		ylabel: "Duration (ms)",
		data: protocolData,
		plot: (dataPath) => `plot '${dataPath}' using 2:xtic(1) title 'TCP', \\
     '' using 3 title 'QUIC (Cubic)', \\
     '' using 4 title 'QUIC (BBR)', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Protocol overhead
	const protocolOverheadData = [10, 100]
		.map((hits) => {
			const tcp = calculateOverhead("faith", "google", false, hits);
			const cubic = calculateOverhead("faith", "google", "cubic", hits);
			const bbr = calculateOverhead("faith", "google", "bbr", hits);
			return `${hits}\t${tcp}\t${cubic}\t${bbr}`;
		})
		.join("\n");

	await generateChart("protocol_overhead", {
		title: "Fáith Protocol Overhead (Google Target - minus x1 baseline)",
		xlabel: "Number of Requests",
		ylabel: "Overhead Duration (ms)",
		data: protocolOverheadData,
		xtics: 'set xtics ("10" 0, "100" 1)',
		plot: (dataPath) => `plot '${dataPath}' using 2:xtic(1) title 'TCP', \\
     '' using 3 title 'QUIC (Cubic)', \\
     '' using 4 title 'QUIC (BBR)', \\
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Variance (box plot)
	const varianceEntries = entries.filter(
		(e) => e.target === "google" && e.hits === 10,
	);
	const varianceGroups = groupBy(
		varianceEntries,
		(e) => `${e.impl}-${e.http3 || "tcp"}`,
		(e) => e.duration,
	);

	const varianceData = Array.from(varianceGroups.entries())
		.map(([key, durations]) => {
			const sorted = [...durations].sort((a, b) => a - b);
			const q1 = sorted[Math.floor(sorted.length * 0.25)];
			const median = sorted[Math.floor(sorted.length * 0.5)];
			const q3 = sorted[Math.floor(sorted.length * 0.75)];
			const min = sorted[0];
			const max = sorted[sorted.length - 1];
			// Get the implementation and protocol from the first entry in the group
			const entry = varianceEntries.find(
				(e) => `${e.impl}-${e.http3 || "tcp"}` === key,
			);
			const label = entry.http3
				? `${entry.impl}-QUIC-${entry.http3}`
				: `${entry.impl}-TCP`;
			return `${min}\t${q1}\t${median}\t${q3}\t${max}\t${label}`;
		})
		.join("\n");

	await generateChart("variance", {
		title: "Performance Variance (Google, 10 requests)",
		xlabel: "",
		ylabel: "Duration (ms)",
		data: varianceData,
		xtics: "",
		extra: "set offsets 0.5, 0.5, 0, 0\nset style fill solid 0.5\nset boxwidth 0.5\nset xtics rotate by -45\nset key off",
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 0:2:1:5:4:xtic(6) with candlesticks whiskerbars lw 2 title 'Min/Max', \\
     '' using 0:3:3:3:3 with candlesticks lw 2 lt -1 notitle`,
	});

	// Packet efficiency
	const packetEffData = [1, 10, 100]
		.map((hits) => {
			const native =
				entries
					.filter(
						(e) =>
							e.impl === "native" &&
							e.target === "local" &&
							e.hits === hits &&
							e.http3 === false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.total || 0)
					.reduce((sum, val) => sum + val, 0) /
				(10 * hits);
			const nodefetch =
				entries
					.filter(
						(e) =>
							e.impl === "node-fetch" &&
							e.target === "local" &&
							e.hits === hits &&
							e.http3 === false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.total || 0)
					.reduce((sum, val) => sum + val, 0) /
				(10 * hits);
			const faithTcp =
				entries
					.filter(
						(e) =>
							e.impl === "faith" &&
							e.target === "local" &&
							e.hits === hits &&
							e.http3 === false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.total || 0)
					.reduce((sum, val) => sum + val, 0) /
				(10 * hits);
			const faithQuic =
				entries
					.filter(
						(e) =>
							e.impl === "faith" &&
							e.target === "google" &&
							e.hits === hits &&
							e.http3 !== false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.total || 0)
					.reduce((sum, val) => sum + val, 0) /
				(20 * hits);
			return `${hits}\t${native}\t${nodefetch}\t${faithTcp}\t${faithQuic}`;
		})
		.join("\n");

	await generateChart("packet_efficiency", {
		title: "Network Efficiency: Packets per Request",
		xlabel: "Number of Requests",
		ylabel: "Average Packets per Request",
		data: packetEffData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC', \\
     '' using ($0-0.33):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.11):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.11):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.33):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Bytes per request
	const bytesPerReqData = [1, 10, 100]
		.map((hits) => {
			const native =
				entries
					.filter(
						(e) =>
							e.impl === "native" &&
							e.target === "local" &&
							e.hits === hits &&
							e.http3 === false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
					.reduce((sum, val) => sum + val, 0) /
				(10 * hits);
			const nodefetch =
				entries
					.filter(
						(e) =>
							e.impl === "node-fetch" &&
							e.target === "local" &&
							e.hits === hits &&
							e.http3 === false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
					.reduce((sum, val) => sum + val, 0) /
				(10 * hits);
			const faithTcp =
				entries
					.filter(
						(e) =>
							e.impl === "faith" &&
							e.target === "local" &&
							e.hits === hits &&
							e.http3 === false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
					.reduce((sum, val) => sum + val, 0) /
				(10 * hits);
			const faithQuic =
				entries
					.filter(
						(e) =>
							e.impl === "faith" &&
							e.target === "google" &&
							e.hits === hits &&
							e.http3 !== false,
					)
					.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
					.reduce((sum, val) => sum + val, 0) /
				(20 * hits);
			return `${hits}\t${native}\t${nodefetch}\t${faithTcp}\t${faithQuic}`;
		})
		.join("\n");

	await generateChart("bytes_per_request", {
		title: "Data Efficiency: Bytes per Request",
		xlabel: "Number of Requests",
		ylabel: "Average Bytes per Request",
		data: bytesPerReqData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC', \\
     '' using ($0-0.33):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.11):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.11):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.33):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Bytes per packet
	const bytesPerPacketData = [1, 10, 100]
		.map((hits) => {
			const nativeEntries = entries.filter(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nativeBytes = nativeEntries
				.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
				.reduce((sum, val) => sum + val, 0);
			const nativePackets = nativeEntries
				.map((e) => packetStatsMap.get(e.filename)?.total || 0)
				.reduce((sum, val) => sum + val, 0);
			const native = nativePackets > 0 ? nativeBytes / nativePackets : 0;

			const nodefetchEntries = entries.filter(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nodefetchBytes = nodefetchEntries
				.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
				.reduce((sum, val) => sum + val, 0);
			const nodefetchPackets = nodefetchEntries
				.map((e) => packetStatsMap.get(e.filename)?.total || 0)
				.reduce((sum, val) => sum + val, 0);
			const nodefetch =
				nodefetchPackets > 0 ? nodefetchBytes / nodefetchPackets : 0;

			const faithTcpEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const faithTcpBytes = faithTcpEntries
				.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithTcpPackets = faithTcpEntries
				.map((e) => packetStatsMap.get(e.filename)?.total || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithTcp =
				faithTcpPackets > 0 ? faithTcpBytes / faithTcpPackets : 0;

			const faithQuicEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.hits === hits &&
					e.http3 !== false,
			);
			const faithQuicBytes = faithQuicEntries
				.map((e) => packetStatsMap.get(e.filename)?.bytes || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithQuicPackets = faithQuicEntries
				.map((e) => packetStatsMap.get(e.filename)?.total || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithQuic =
				faithQuicPackets > 0 ? faithQuicBytes / faithQuicPackets : 0;

			return `${hits}\t${native}\t${nodefetch}\t${faithTcp}\t${faithQuic}`;
		})
		.join("\n");

	await generateChart("bytes_per_packet", {
		title: "Network Efficiency: Average Bytes per Packet",
		xlabel: "Number of Requests",
		ylabel: "Bytes per Packet",
		data: bytesPerPacketData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC', \\
     '' using ($0-0.33):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.11):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.11):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.33):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Throughput
	const throughputData = [1, 10, 100]
		.map((hits) => {
			const native =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "native" &&
						e.target === "google" &&
						e.http3 === false &&
						e.hits === hits,
				) /
					1000);
			const nodefetch =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "node-fetch" &&
						e.target === "google" &&
						e.http3 === false &&
						e.hits === hits,
				) /
					1000);
			const faithTcp =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "faith" &&
						e.target === "google" &&
						e.http3 === false &&
						e.hits === hits,
				) /
					1000);
			const faithCubic =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "faith" &&
						e.target === "google" &&
						e.http3 === "cubic" &&
						e.hits === hits,
				) /
					1000);
			const faithBbr =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "faith" &&
						e.target === "google" &&
						e.http3 === "bbr" &&
						e.hits === hits,
				) /
					1000);
			return `${hits}\t${native}\t${nodefetch}\t${faithTcp}\t${faithCubic}\t${faithBbr}`;
		})
		.join("\n");

	await generateChart("throughput_google", {
		title: "Throughput: Requests per Second (Google Target)",
		xlabel: "Number of Requests",
		ylabel: "Requests/Second",
		data: throughputData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC (Cubic)', \\
     '' using 6 title 'Fáith-QUIC (BBR)', \\
     '' using ($0-0.4):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.2):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.2):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.4):6:(sprintf("%.1f",$6)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Throughput (local)
	const throughputLocalData = [1, 10, 100]
		.map((hits) => {
			const native =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "native" &&
						e.target === "local" &&
						e.http3 === false &&
						e.hits === hits,
				) /
					1000);
			const nodefetch =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "node-fetch" &&
						e.target === "local" &&
						e.http3 === false &&
						e.hits === hits,
				) /
					1000);
			const faith =
				hits /
				(getAvgDuration(
					(e) =>
						e.impl === "faith" &&
						e.target === "local" &&
						e.http3 === false &&
						e.hits === hits,
				) /
					1000);
			return `${hits}\t${native}\t${nodefetch}\t${faith}`;
		})
		.join("\n");

	await generateChart("throughput_local", {
		title: "Throughput: Requests per Second (Local Target)",
		xlabel: "Number of Requests",
		ylabel: "Requests/Second",
		data: throughputLocalData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith', \\
     '' using ($0-0.27):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.27):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Connections per request
	const connectionsPerReqData = [1, 10, 100]
		.map((hits) => {
			const nativeEntries = entries.filter(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nativeConns = nativeEntries
				.map((e) => packetStatsMap.get(e.filename)?.connections || 0)
				.reduce((sum, val) => sum + val, 0);
			const native = nativeConns / nativeEntries.length;

			const nodefetchEntries = entries.filter(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nodefetchConns = nodefetchEntries
				.map((e) => packetStatsMap.get(e.filename)?.connections || 0)
				.reduce((sum, val) => sum + val, 0);
			const nodefetch = nodefetchConns / nodefetchEntries.length;

			const faithTcpEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const faithTcpConns = faithTcpEntries
				.map((e) => packetStatsMap.get(e.filename)?.connections || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithTcp = faithTcpConns / faithTcpEntries.length;

			const faithQuicEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.hits === hits &&
					e.http3 !== false,
			);
			const faithQuicConns = faithQuicEntries
				.map((e) => packetStatsMap.get(e.filename)?.connections || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithQuic = faithQuicConns / faithQuicEntries.length;

			return `${hits}\t${native}\t${nodefetch}\t${faithTcp}\t${faithQuic}`;
		})
		.join("\n");

	await generateChart("connections_per_request", {
		title: "Connection Reuse: Total Connections",
		xlabel: "Number of Requests",
		ylabel: "Total Connections",
		data: connectionsPerReqData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC', \\
     '' using ($0-0.33):2:(sprintf("%.2f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.11):3:(sprintf("%.2f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.11):4:(sprintf("%.2f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.33):5:(sprintf("%.2f",$5)) with labels center offset 0,1 font ",8" notitle`,
	});

	// DNS requests
	const dnsData = [1, 10, 100]
		.map((hits) => {
			const nativeEntries = entries.filter(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nativeDns = nativeEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsQueries || 0)
				.reduce((sum, val) => sum + val, 0);
			const native = nativeDns / nativeEntries.length;

			const nodefetchEntries = entries.filter(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nodefetchDns = nodefetchEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsQueries || 0)
				.reduce((sum, val) => sum + val, 0);
			const nodefetch = nodefetchDns / nodefetchEntries.length;

			const faithTcpEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const faithTcpDns = faithTcpEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsQueries || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithTcp = faithTcpDns / faithTcpEntries.length;

			const faithQuicEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.hits === hits &&
					e.http3 !== false,
			);
			const faithQuicDns = faithQuicEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsQueries || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithQuic = faithQuicDns / faithQuicEntries.length;

			return `${hits}\t${native}\t${nodefetch}\t${faithTcp}\t${faithQuic}`;
		})
		.join("\n");

	await generateChart("dns_queries", {
		title: "DNS Resolution: DNS Queries",
		xlabel: "Number of Requests",
		ylabel: "DNS Queries",
		data: dnsData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC', \\
     '' using ($0-0.33):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.11):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.11):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.33):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle`,
	});

	// DNS responses
	const dnsResponseData = [1, 10, 100]
		.map((hits) => {
			const nativeEntries = entries.filter(
				(e) =>
					e.impl === "native" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nativeDnsResp = nativeEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsResponses || 0)
				.reduce((sum, val) => sum + val, 0);
			const native = nativeDnsResp / nativeEntries.length;

			const nodefetchEntries = entries.filter(
				(e) =>
					e.impl === "node-fetch" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const nodefetchDnsResp = nodefetchEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsResponses || 0)
				.reduce((sum, val) => sum + val, 0);
			const nodefetch = nodefetchDnsResp / nodefetchEntries.length;

			const faithTcpEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "local" &&
					e.hits === hits &&
					e.http3 === false,
			);
			const faithTcpDnsResp = faithTcpEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsResponses || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithTcp = faithTcpDnsResp / faithTcpEntries.length;

			const faithQuicEntries = entries.filter(
				(e) =>
					e.impl === "faith" &&
					e.target === "google" &&
					e.hits === hits &&
					e.http3 !== false,
			);
			const faithQuicDnsResp = faithQuicEntries
				.map((e) => packetStatsMap.get(e.filename)?.dnsResponses || 0)
				.reduce((sum, val) => sum + val, 0);
			const faithQuic = faithQuicDnsResp / faithQuicEntries.length;

			return `${hits}\t${native}\t${nodefetch}\t${faithTcp}\t${faithQuic}`;
		})
		.join("\n");

	await generateChart("dns_responses", {
		title: "DNS Resolution: DNS Responses",
		xlabel: "Number of Requests",
		ylabel: "DNS Responses",
		data: dnsResponseData,
		plot: (
			dataPath,
		) => `plot '${dataPath}' using 2:xtic(1) title 'native', \\
     '' using 3 title 'node-fetch', \\
     '' using 4 title 'Fáith-TCP', \\
     '' using 5 title 'Fáith-QUIC', \\
     '' using ($0-0.33):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0-0.11):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.11):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \\
     '' using ($0+0.33):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle`,
	});

	// Discover all available targets
	const allTargets = [...new Set(entries.map((e) => e.target))].sort();
	console.log(`\nDiscovered targets: ${allTargets.join(", ")}`);

	// Cross-target comparison for Fáith (100 requests, TCP)
	const crossTargetData = allTargets
		.map((target) => {
			const duration = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === target &&
					e.http3 === false &&
					e.hits === 100,
			);
			return duration > 0 ? `${target}\t${Math.round(duration)}` : null;
		})
		.filter((x) => x !== null)
		.join("\n");

	if (crossTargetData) {
		await generateChart("cross_target_faith", {
			title: "Fáith Performance Across Targets (100 requests, TCP)",
			xlabel: "Target",
			ylabel: "Duration (ms)",
			data: crossTargetData,
			xtics: "",
			plot: (
				dataPath,
			) => `plot '${dataPath}' using 2:xtic(1) title 'Fáith' with boxes, \\
     '' using 0:2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle`,
		});
	}

	// All implementations comparison (100 requests, TCP) for each target
	for (const target of allTargets) {
		const targetData = ["native", "node-fetch", "faith"]
			.map((impl) => {
				const duration = getAvgDuration(
					(e) =>
						e.impl === impl &&
						e.target === target &&
						e.http3 === false &&
						e.hits === 100,
				);
				return duration > 0 ? `${impl}\t${Math.round(duration)}` : null;
			})
			.filter((x) => x !== null)
			.join("\n");

		if (targetData) {
			const safeTarget = target.replace(/[^a-z0-9]/gi, "_");
			await generateChart(`impl_comparison_${safeTarget}`, {
				title: `Implementation Comparison: ${target} (100 requests, TCP)`,
				xlabel: "Implementation",
				ylabel: "Duration (ms)",
				data: targetData,
				xtics: "",
				plot: (
					dataPath,
				) => `plot '${dataPath}' using 2:xtic(1) title 'Duration' with boxes, \\
     '' using 0:2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle`,
			});
		}
	}

	// Throughput comparison across all targets (Fáith only, 100 requests)
	const throughputAllTargetsData = allTargets
		.map((target) => {
			// Try TCP first
			let duration = getAvgDuration(
				(e) =>
					e.impl === "faith" &&
					e.target === target &&
					e.http3 === false &&
					e.hits === 100,
			);
			// Fall back to any protocol if TCP not available
			if (duration === 0) {
				duration = getAvgDuration(
					(e) =>
						e.impl === "faith" &&
						e.target === target &&
						e.hits === 100,
				);
			}
			const throughput =
				duration > 0 ? (100 / (duration / 1000)).toFixed(1) : 0;
			return throughput > 0 ? `${target}\t${throughput}` : null;
		})
		.filter((x) => x !== null)
		.join("\n");

	if (throughputAllTargetsData) {
		await generateChart("throughput_all_targets", {
			title: "Fáith Throughput Across All Targets (100 requests)",
			xlabel: "Target",
			ylabel: "Requests/Second",
			data: throughputAllTargetsData,
			xtics: "",
			plot: (
				dataPath,
			) => `plot '${dataPath}' using 2:xtic(1) title 'Fáith' with boxes, \\
     '' using 0:2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle`,
		});
	}

	// Generate summary report
	console.log("\n=== BENCHMARK SUMMARY ===\n");

	const scenarios = [
		{
			label: "Local 1 request",
			filter: (e) => e.target === "local" && e.hits === 1,
		},
		{
			label: "Local 100 requests",
			filter: (e) => e.target === "local" && e.hits === 100,
		},
		{
			label: "Google 1 request (TCP)",
			filter: (e) =>
				e.target === "google" && e.hits === 1 && e.http3 === false,
		},
		{
			label: "Google 100 requests (TCP)",
			filter: (e) =>
				e.target === "google" && e.hits === 100 && e.http3 === false,
		},
	];

	console.log("Top performers by scenario:\n");
	for (const { label, filter } of scenarios) {
		const byImpl = groupBy(
			entries.filter(filter),
			(e) => e.impl,
			(e) => e.duration,
		);
		const results = Array.from(byImpl.entries())
			.map(([impl, durations]) => ({
				impl,
				mean: calculateStats(durations).mean,
			}))
			.sort((a, b) => a.mean - b.mean);
		if (results.length > 0) {
			console.log(`${label}:`);
			console.log(
				`  ${results[0].impl}: ${Math.round(results[0].mean)} ms`,
			);
		}
	}

	const faithQuic = groupBy(
		entries.filter(
			(e) =>
				e.impl === "faith" &&
				e.target === "google" &&
				e.hits === 100 &&
				e.http3,
		),
		(e) => e.http3,
		(e) => e.duration,
	);
	const faithQuicResults = Array.from(faithQuic.entries())
		.map(([proto, durations]) => ({
			proto,
			mean: calculateStats(durations).mean,
		}))
		.sort((a, b) => a.mean - b.mean);
	if (faithQuicResults.length > 0) {
		console.log("\nFaith QUIC best performer (Google, 100 requests):");
		console.log(
			`  QUIC (${faithQuicResults[0].mean}): ${Math.round(faithQuicResults[0].proto)} ms`,
		);
	}

	console.log("\n=== STATISTICAL SUMMARY ===\n");
	console.log(
		"Mean durations by implementation (Google, 10 requests, TCP):\n",
	);
	const google10 = groupBy(
		entries.filter(
			(e) => e.target === "google" && e.hits === 10 && e.http3 === false,
		),
		(e) => e.impl,
		(e) => e.duration,
	);
	for (const [impl, durations] of google10) {
		const stats = calculateStats(durations);
		console.log(
			`  ${impl}: ${Math.round(stats.mean)} ms ± ${Math.round(stats.stddev)} ms`,
		);
	}

	// Save packet stats
	const packetStatsTxt = [
		"# filename total_packets tcp_packets udp_packets total_bytes connections dns_queries dns_responses",
		...packetStats.map(
			(s) =>
				`${s.filename} ${s.total} ${s.tcp} ${s.udp} ${s.bytes} ${s.connections} ${s.dnsQueries} ${s.dnsResponses}`,
		),
	].join("\n");
	await writeFile(`${OUTPUT_DIR}/packet_stats.txt`, packetStatsTxt);

	// Save summary statistics
	const allStats = Array.from(
		groupBy(
			entries,
			(e) => `${e.impl}-${e.target}-x${e.hits}-${e.http3 || "tcp"}`,
			(e) => e,
		),
	).map(([key, items]) => {
		const durations = items.map((e) => e.duration);
		return {
			key,
			...items[0],
			count: items.length,
			...calculateStats(durations),
		};
	});
	await writeFile(
		`${OUTPUT_DIR}/stats.json`,
		JSON.stringify(allStats, null, 2),
	);

	console.log(`\nPacket statistics saved to ${OUTPUT_DIR}/packet_stats.txt`);
	console.log(`Statistics saved to ${OUTPUT_DIR}/stats.json`);
	console.log("\nDone!");
}

main().catch((err) => {
	console.error("Error:", err);
	process.exit(1);
});
