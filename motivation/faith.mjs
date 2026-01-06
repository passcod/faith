import { fetch, Agent } from "@passcod/faith";

const agent = new Agent({ http3: { upgradeEnabled: !!process.env.HTTP3 } });

const TARGET = process.env.TARGET;
const HITS = process.env.HITS;
let n = 0;

while (n < HITS) {
	const resp = await fetch(TARGET, { agent });
	await resp.discard();
	n += 1;
}
