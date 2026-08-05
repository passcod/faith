/**
 * The renderer is what someone reading the README actually sees, so a mistake here
 * misreports coverage to every reader without any test going red. It takes plain
 * data and returns a string, so it can be checked directly.
 */

const test = require("tape");

const { render } = require("./render.js");

function cell(server, dimension, outcome, reason = null) {
	return { server, dimension, status: outcome === "skipped" ? "skip" : "run", reason, outcome };
}

test("render: refuses a matrix that carries no outcomes", (t) => {
	t.throws(
		() => render({ kind: "planned", cells: [] }),
		/refusing to render/,
		"a planned matrix is rejected rather than drawn as though it had passed",
	);
	t.end();
});

test("render: every outcome is distinguishable", (t) => {
	const table = render({
		kind: "realised",
		cells: [
			cell("alpha", "one", "pass"),
			cell("alpha", "two", "fail"),
			cell("beta", "one", "skipped", "lacks trailers"),
			cell("beta", "two", "unavailable"),
		],
	});

	const marks = new Set(table.match(/[✅❌—⬜]/gu));
	t.equal(marks.size, 4, "four outcomes draw four different marks");
	t.ok(/ran and passed/.test(table), "and the legend explains the ones present");
	t.ok(/not installed/.test(table), "including unavailable, which is not a skip");
	t.end();
});

test("render: the legend covers exactly what the table contains", (t) => {
	const green = render({ kind: "realised", cells: [cell("alpha", "one", "pass")] });
	t.ok(/ran and passed/.test(green), "an all-passing table explains the pass mark");
	t.notOk(
		/ran and failed/.test(green),
		"and says nothing about failure, which would make it look worse than it is",
	);
	t.end();
});

test("render: repeated skip reasons share one footnote", (t) => {
	const table = render({
		kind: "realised",
		cells: [
			cell("alpha", "one", "skipped", "lacks chunked"),
			cell("beta", "one", "skipped", "lacks chunked"),
			cell("alpha", "two", "skipped", "lacks trailers"),
		],
	});

	t.equal(
		(table.match(/^<sup>\d+<\/sup> lacks/gmu) || []).length,
		2,
		"two distinct reasons, not three footnotes for three cells",
	);
	t.ok(/<sup>1<\/sup> lacks chunked/.test(table), "and the shared one is numbered once");
	t.end();
});

test("render: what it cannot explain, it does not hide", (t) => {
	// Both cases mean the renderer and the matrix disagree. Drawing a blank would
	// read as "nothing to report here", which is the one thing it must not say.
	const hole = render({ kind: "realised", cells: [cell("alpha", "one", "pass"), cell("beta", "two", "pass")] });
	t.ok(/\| \?/.test(hole), "a cell the matrix never described is marked, not blanked");

	const unknown = render({ kind: "realised", cells: [cell("alpha", "one", "invented")] });
	t.ok(/`invented`/.test(unknown), "an outcome it has no mark for is printed verbatim");
	t.end();
});
