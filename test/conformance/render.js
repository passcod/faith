/**
 * Renders `matrix.json` into one table, used both for the README and as a CI
 * artifact.
 *
 * One table, because there is nothing to tell the two audiences apart: nothing merges
 * with a failing matrix, so a reader of the README on `main` sees the same grid a
 * green run produced. The failure and not-run marks exist for the artifact of a red
 * run, where the README is not regenerated at all.
 *
 * No footnotes. Which capability a row lacks is in `matrix.json`, uploaded beside
 * this, and eleven numbered notes saying "this combination does not apply" is not a
 * README.
 *
 * `--readme` writes the table into the README. CI regenerates it and lets
 * `git diff --exit-code` do the rest: a failing matrix has already failed the job by
 * then, and git prints a better patch than this could.
 */

const { readFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");

const MATRIX_PATH = path.join(__dirname, "matrix.json");
const OUTPUT_PATH = path.join(__dirname, "matrix.md");
const README_PATH = path.join(__dirname, "..", "..", "README.md");

const START = "<!-- conformance:start -->";
const END = "<!-- conformance:end -->";

/** What each outcome looks like, and what it means underneath the table. */
const OUTCOMES = {
	pass: { mark: "●", legend: "covered" },
	skipped: { mark: "·", legend: "not applicable to this server" },
	fail: { mark: "❌", legend: "ran and failed" },
	unavailable: { mark: "⬜", legend: "not run: the server was unavailable here" },
};

function render({ kind, cells }) {
	if (kind !== "realised") {
		throw new Error(
			`refusing to render a "${kind}" matrix: only a realised one carries outcomes, ` +
				`and a table of intentions is indistinguishable from a table of results`,
		);
	}

	// Dimensions as rows and servers as columns: dimensions are the shorter list and
	// grow more often, so this keeps the table from getting wider than a README.
	const servers = unique(cells.map((c) => c.server));
	const dimensions = unique(cells.map((c) => c.dimension));
	const byKey = new Map(cells.map((c) => [`${c.server} ${c.dimension}`, c]));

	const lines = [
		`| | ${servers.join(" | ")} |`,
		`| --- | ${servers.map(() => "---").join(" | ")} |`,
	];
	for (const dimension of dimensions) {
		const row = servers.map((server) => {
			const cell = byKey.get(`${server} ${dimension}`);
			// A hole means this renderer and the matrix disagree about which cells exist,
			// and a blank would read as "nothing to report here".
			if (!cell) return "?";
			const outcome = OUTCOMES[cell.outcome];
			return outcome ? outcome.mark : `\`${cell.outcome}\``;
		});
		lines.push(`| **${dimension}** | ${row.join(" | ")} |`);
	}

	// Only the outcomes actually present: a legend explaining ❌ under an all-green
	// table teaches the reader nothing and makes the table look worse than it is.
	const present = new Set(cells.map((c) => c.outcome));
	lines.push("");
	// Fenced, so each entry keeps its own line. A markdown hard break needs two trailing
	// spaces, which formatters strip; joining them on a separator put a middle dot between
	// entries, which is also the not-applicable mark, and "● covered · · not applicable" is
	// not something to make a reader parse.
	lines.push("```");
	for (const [name, { mark, legend }] of Object.entries(OUTCOMES)) {
		if (present.has(name)) lines.push(`${mark} ${legend}`);
	}
	lines.push("```");
	return `${lines.join("\n")}\n`;
}

function unique(values) {
	return [...new Set(values)];
}

/** The README with its conformance block replaced. Throws if the markers are gone. */
function injectReadme(readme, table) {
	const start = readme.indexOf(START);
	const end = readme.indexOf(END);
	if (start === -1 || end === -1 || end < start) {
		throw new Error(`README.md is missing the ${START} / ${END} markers`);
	}
	return `${readme.slice(0, start + START.length)}\n\n${table}\n${readme.slice(end)}`;
}

function main(argv) {
	const table = render(JSON.parse(readFileSync(MATRIX_PATH, "utf8")));

	if (!argv.includes("--readme")) {
		writeFileSync(OUTPUT_PATH, table);
		console.log(`wrote ${path.relative(process.cwd(), OUTPUT_PATH)}`);
		return;
	}

	writeFileSync(README_PATH, injectReadme(readFileSync(README_PATH, "utf8"), table));
	console.log("updated the conformance table in README.md");
}

if (require.main === module) {
	try {
		main(process.argv.slice(2));
	} catch (err) {
		// A stack trace would bury the one sentence that says what to do about it.
		console.error(err.message);
		process.exitCode = 1;
	}
}

module.exports = { render, injectReadme, OUTCOMES, START, END };
