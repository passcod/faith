#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";

const targets = [
	{ name: "google", url: "https://www.google.com/" },
	// { name: "google-redirect", url: "https://google.com/" },
	// { name: "local", url: "http://10.88.0.30:8080" },
];
const hitses = [1, 10, 100];
const h3 = [false, "cubic", "bbr"];
const impl = [
	{ name: "native", cmd: ["node", "native-fetch.js"] },
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
// try {
// 	const extant = JSON.parse(await readFile("bench-data-node.json"));
// 	for (const value of extant) {
// 		data.set(value.filename, value);
// 	}
// } catch (_) {}
// try {
// 	const extant = JSON.parse(await readFile("bench-data-faith.json"));
// 	for (const value of extant) {
// 		data.set(value.filename, value);
// 	}
// } catch (_) {}

for (const { name: target, url } of targets) {
	for (const { name, cmd } of impl) {
		for (const http3 of h3) {
			if (http3 && target === "local") continue;
			if (http3 && (name === "native" || name === "node-fetch")) continue;

			for (const hits of hitses) {
				for (let n = 0; n < 10; n += 1) {
					const filename = `${name}-${target}-x${hits}-${http3 ? `quic-${http3}` : "tcp"}-${n}`;
					const start = performance.now();
					try {
						execFileSync(
							"./netrace.sh",
							[`${filename}.pcap`, ...cmd],
							{
								env: {
									...process.env,
									TARGET: url,
									HITS: hits.toString(),
									HTTP3: http3,
								},
							},
						);
						const duration = performance.now() - start;

						data.set(filename, {
							impl: name,
							http3,
							hits,
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
			}
		}
	}
}

await writeFile(
	"bench-data.json",
	JSON.stringify(Object.fromEntries(data.entries()), null, 2),
);
