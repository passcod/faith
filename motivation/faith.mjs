import { fetch, Agent } from "@passcod/faith";

const TARGET = process.env.TARGET;
const HTTP3 = process.env.HTTP3;
const HITS = process.env.HITS;
const SEQ = process.env.SEQ ? parseInt(process.env.SEQ, 10) : 1;

const url = new URL(TARGET);
const host = url.hostname;

const agent = new Agent({
	http3: {
		upgradeEnabled: !!HTTP3,
		congestion: HTTP3 === "bbr" ? "bbr1" : "cubic",
		hints: HTTP3 ? [{ host, port: 443 }] : [],
	},
});

let n = 0;
while (n < HITS) {
	const batch = [];
	for (let i = 0; i < SEQ && n < HITS; i++, n++) {
		batch.push(fetch(TARGET, { agent }).then((resp) => resp.discard()));
	}
	await Promise.all(batch);
}
