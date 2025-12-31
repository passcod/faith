const TARGET = process.env.TARGET;
const HITS = process.env.HITS;
let n = 0;

while (n < HITS) {
	await fetch(TARGET);
	n += 1;
}
