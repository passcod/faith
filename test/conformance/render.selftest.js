/**
 * The renderer is what someone reading the README actually sees, so a mistake here
 * misreports coverage to every reader without any test going red. It takes plain
 * data and returns a string, so it can be checked directly.
 */

const test = require("tape");

const { render, injectReadme, START, END } = require("./render.js");

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

	const marks = new Set(table.match(/[●·❌⬜]/gu));
	t.equal(marks.size, 4, "four outcomes draw four different marks");
	t.ok(/covered/.test(table), "and the legend explains the ones present");
	t.ok(/unavailable here/.test(table), "including not-run, which is not an exclusion");
	t.end();
});

test("render: the legend covers exactly what the table contains", (t) => {
	const green = render({
		kind: "realised",
		cells: [cell("alpha", "one", "pass"), cell("beta", "one", "skipped", "lacks gzip")],
	});

	t.ok(/● covered/.test(green), "an all-passing table explains the covered mark");
	t.notOk(
		/ran and failed/.test(green),
		"and says nothing about failure, which would make it look worse than it is",
	);
	// One per line: joining them on a separator would put a middle dot between
	// entries, which is also the not-applicable mark.
	t.equal(green.trimEnd().split("\n").slice(-2).length, 2, "each present mark gets its own line");
	t.end();
});

test("render: no footnotes, whatever the reasons are", (t) => {
	const table = render({
		kind: "realised",
		cells: [
			cell("alpha", "one", "skipped", "lacks chunked"),
			cell("beta", "one", "skipped", "lacks trailers"),
			cell("gamma", "one", "skipped", "lacks gzip"),
		],
	});

	t.notOk(/<sup>/.test(table), "three distinct reasons produce no numbered notes");
	t.notOk(/lacks/.test(table), "and the reasons stay in matrix.json, which ships beside this");
	t.end();
});

test("render: what it cannot explain, it does not hide", (t) => {
	// Both cases mean the renderer and the matrix disagree. Drawing a blank would read
	// as "nothing to report here", which is the one thing it must not say.
	const hole = render({
		kind: "realised",
		cells: [cell("alpha", "one", "pass"), cell("beta", "two", "pass")],
	});
	t.ok(/\| \?/.test(hole), "a cell the matrix never described is marked, not blanked");

	const unknown = render({ kind: "realised", cells: [cell("alpha", "one", "invented")] });
	t.ok(/`invented`/.test(unknown), "an outcome it has no mark for is printed verbatim");
	t.end();
});

test("render: injecting replaces the block and nothing else", (t) => {
	const readme = `# title\n\nbefore\n\n${START}\nstale\n${END}\n\nafter\n`;
	const injected = injectReadme(readme, "fresh\n");

	t.ok(injected.includes("fresh"), "the new table lands between the markers");
	t.notOk(injected.includes("stale"), "and the old one is gone");
	t.ok(injected.startsWith("# title\n\nbefore"), "everything before is untouched");
	t.ok(injected.endsWith("after\n"), "and so is everything after");
	// Idempotent, or --check would report a diff against a table it had just written.
	t.equal(injectReadme(injected, "fresh\n"), injected, "injecting twice changes nothing");
	t.end();
});

test("render: a README without markers is an error, not a silent no-op", (t) => {
	t.throws(
		() => injectReadme("# title\n\nno markers here\n", "fresh\n"),
		/missing the/,
		"because silently writing nothing would leave a stale table looking current",
	);
	t.end();
});
