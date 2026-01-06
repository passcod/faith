#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";

const targets = [
	// { name: "local", url: "http://10.88.0.30:8080" },
	// { name: "google", url: "https://www.google.com/" },
	{
		name: "cloudflare-100k",
		url: "https://speed.cloudflare.com/__down?bytes=100000",
	},
];
const hitses = [1, 10, 100];
const h3 = [false, "cubic", "bbr"];
const impl = [
	// { name: "native", cmd: ["node", "native-fetch.js"] },
	{ name: "node-fetch", cmd: ["node", "node-fetch.js"] },
	{ name: "faith", cmd: ["node", "faith.mjs"] },
];

// probe for if the script works
execFileSync("./netrace.sh", ["probe.pcap", "node", "faith.mjs"], {
	env: {
		...process.env,
		TARGET: "http://localhost",
		HITS: 0,
	},
});

const data = new Map();
try {
	const extant = JSON.parse(await readFile("bench-data.json"));
	for (const [key, value] of Object.entries(extant)) {
		data.set(key, value);
	}
} catch (_) {}

for (const { name: target, url } of targets) {
	for (const { name, cmd } of impl) {
		for (const http3 of h3) {
			if (
				http3 &&
				(target === "local" ||
					target === "cloudflare-100k" ||
					name === "native" ||
					name === "node-fetch")
			)
				continue;

			for (const hits of hitses) {
				const seqValues = hits === 100 ? [1, 10, 25, 50] : [undefined];

				for (const seq of seqValues) {
					for (let n = 0; n < 10; n += 1) {
						const seqSuffix = seq !== undefined ? `-seq${seq}` : "";
						const filename = `${name}-${target}-x${hits}-${http3 ? `quic-${http3}` : "tcp"}${seqSuffix}-${n}`;
						const start = performance.now();
						try {
							const env = {
								...process.env,
								TARGET: url,
								HITS: hits.toString(),
								HTTP3: http3,
							};
							if (seq !== undefined) {
								env.SEQ = seq.toString();
							}

							execFileSync(
								"./netrace.sh",
								[`data/${filename}.pcap`, ...cmd],
								{ env },
							);
							const duration = performance.now() - start;

							data.set(filename, {
								impl: name,
								http3,
								hits,
								seq,
								target,
								url,
								n,
								filename,
								duration,
							});
							console.log(`${filename} ${duration}ms`);
						} catch (err) {
							console.log(`${filename} ERR: ${err}`);
						}
					}

					await writeFile(
						"bench-data.json",
						JSON.stringify(
							Object.fromEntries(data.entries()),
							null,
							2,
						),
					);
				}
			}
		}
	}
}
