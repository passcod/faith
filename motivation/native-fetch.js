const TARGET = process.env.TARGET;
const HITS = process.env.HITS;
const SEQ = process.env.SEQ ? parseInt(process.env.SEQ, 10) : 1;

let n = 0;
while (n < HITS) {
	const batch = [];
	for (let i = 0; i < SEQ && n < HITS; i++, n++) {
		batch.push(fetch(TARGET).then((resp) => resp.bytes()));
	}
	await Promise.all(batch);
}
