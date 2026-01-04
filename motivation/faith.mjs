import { fetch } from "@passcod/faith";

const TARGET = process.env.TARGET;
const HITS = process.env.HITS;
let n = 0;

while (n < HITS) {
	const resp = await fetch(TARGET);
	await resp.discard();
	n += 1;
}
