/**
 * Computes and runs the conformance matrix.
 *
 * The matrix is derived, not written down: each server declares its
 * capabilities, each dimension declares its requirements, and a cell runs only
 * when the requirements are met. A hand-maintained matrix would drift from what
 * the servers can actually do the first time a build changed.
 *
 * Emits `matrix.json` alongside the human-readable output, so the matrix can
 * later be rendered into the README without re-deriving it.
 */

const test = require("tape");
const { writeFileSync } = require("node:fs");
const path = require("node:path");

const { CAPABILITIES: C, assertKnownCapabilities } = require("./capabilities.js");
const { controllableH1, controllableH2 } = require("./servers/controllable.js");

const { Agent } = require("../../index.js");
const { fetch } = require("../../wrapper.js");

const SERVERS = [controllableH1, controllableH2];
const DIMENSIONS = [
	require("./dimensions/trailers.js"),
	require("./dimensions/framing.js"),
	require("./dimensions/encoding.js"),
];

const EXPECTED = require("./expected-matrix.json");

/**
 * Per-cell ceiling. Generous: a cell makes a handful of loopback requests. Its
 * job is to turn a wedged cell into a named failure rather than a silent CI
 * timeout.
 */
const CELL_TIMEOUT_MS = 30_000;

function planCells() {
	// Each dimension's requirements are static, so validating them once here
	// -- rather than once per server inside the nested loop below -- avoids
	// re-checking the same data redundantly.
	for (const dimension of DIMENSIONS) {
		assertKnownCapabilities(dimension.requires, `dimension ${dimension.name}`);
	}

	const cells = [];
	for (const server of SERVERS) {
		assertKnownCapabilities(server.capabilities, `server ${server.name}`);
		for (const dimension of DIMENSIONS) {
			const missing = dimension.requires.filter((c) => !server.capabilities.has(c));
			// Carry the objects themselves, not their names. Re-finding them by name
			// later would mean two dimensions sharing a `name` -- a copy-pasted
			// module with the name unchanged -- silently ran one twice and the other
			// never, while the guard saw two identical cells and passed. Names are
			// derived from these only for serialisation and comparison.
			cells.push({
				server,
				dimension,
				status: missing.length === 0 ? "run" : "skip",
				reason: missing.length === 0 ? null : `lacks ${missing.join(", ")}`,
			});
		}
	}
	return cells;
}

/** A cell as it is written down and compared: names, not object references. */
function serialiseCell({ server, dimension, status, reason }) {
	return { server: server.name, dimension: dimension.name, status, reason };
}

/**
 * A cell plus what actually happened to it.
 *
 * `status` is what the capability model decided; `outcome` is what the run did.
 * They answer different questions -- a cell can be `run` and still `fail`, which
 * is exactly the case a consumer must not mistake for verified.
 */
function serialiseOutcome(cell) {
	return {
		...serialiseCell(cell),
		outcome: cell.status === "skip" ? "skipped" : cell.failed ? "fail" : "pass",
	};
}

const MATRIX_PATH = path.join(__dirname, "matrix.json");

/**
 * Write the matrix out. `kind` tells a reader which one they have: `planned` is
 * written before the run so the file survives a crash, `realised` replaces it
 * once outcomes are known.
 */
function writeMatrix(kind, cells) {
	writeFileSync(MATRIX_PATH, `${JSON.stringify({ kind, cells }, null, "\t")}\n`);
}

async function main() {
	const cells = planCells();

	// A per-cell timeout reports a wedged cell but does not end the run: a
	// dimension awaiting something that never settles (faith's `trailers` spins
	// until the body is drained) leaves a pending native future holding the event
	// loop open, so the process would print the failure and then hang anyway.
	// Forcing the exit is safe here because run.js is a dedicated entry point, not
	// part of the `tape test/*.test.js` glob.
	let anyFailed = false;
	test.onFailure(() => {
		anyFailed = true;
	});
	test.onFinish(() => {
		// The realised matrix, written once the outcomes are known. The runner is the
		// only thing that knows them, so emitting them is its job; whatever renders
		// this into the README is a consumer and must not have to re-derive or re-run
		// anything to find out what actually happened.
		writeMatrix("realised", cells.map(serialiseOutcome));

		if (anyFailed) {
			// tape writes the plan and the summary counts from its own `exit` hook,
			// but that hook returns early when the exit code is non-zero -- so
			// force-exiting with 1 would emit failures and then no `1..n` line at
			// all, which a TAP parser reads as "no tests ran, fine". Closing here
			// writes them; tape then skips its own close, so nothing double-closes.
			test.getHarness().close();
		}
		process.exit(anyFailed ? 1 : 0);
	});

	const serialised = cells.map(serialiseCell);

	// Write the planned matrix up front so the file exists even if the process dies
	// mid-run, then overwrite it with the realised one from `onFinish` above. A
	// reader can tell which it got from `kind`, so a crashed run is never mistaken
	// for a completed one.
	writeMatrix("planned", serialised);

	// Sort both sides: the guard is about which cells exist and their status, not
	// the order SERVERS and DIMENSIONS happen to be declared in. Comparing
	// declaration order would make alphabetising a list, or inserting a dimension
	// rather than appending one, fail indistinguishably from a real regression.
	const byCell = (a, b) =>
		a.server.localeCompare(b.server) || a.dimension.localeCompare(b.dimension);

	test("conformance: planned matrix matches the expected one", (t) => {
		// A cell silently disappearing -- because a capability declaration
		// changed, or a server stopped starting -- looks identical to a clean run
		// unless the shape itself is asserted. The skip reason is compared too: a
		// cell that skips for a *different* reason than intended is coverage
		// quietly reduced, and a status-only comparison accepts it.
		t.deepEqual(
			[...serialised].sort(byCell),
			[...EXPECTED.cells].sort(byCell),
			"no cell appeared, vanished or changed its reason unnoticed",
		);
		t.end();
	});

	for (const cell of cells) {
		const { server, dimension } = cell;

		if (cell.status === "skip") {
			test(`${server.name} / ${dimension.name}`, (t) => {
				t.pass(`skipped: ${cell.reason}`);
				t.end();
			});
			continue;
		}

		test(
			`${server.name} / ${dimension.name}`,
			{ timeout: CELL_TIMEOUT_MS },
			async (t) => {
				// Record the cell's own outcome for the realised matrix. tape's
				// onFailure is global, so it cannot say *which* cell failed.
				t.on("result", (row) => {
					if (row && row.ok === false) cell.failed = true;
				});

				let running;
				try {
					running = await server.start();
				} catch (err) {
					// Loud, not skipped: a server that will not start is a failure of
					// the row, and its own log is the only useful diagnostic.
					t.fail(`${server.name} failed to start: ${err.message}`);
					t.end();
					return;
				}

				const agent = new Agent({
					tls: { extraRoots: [running.ca] },
					dns: { overrides: [{ domain: "localhost", addresses: ["127.0.0.1"] }] },
					http3: { upgradeEnabled: false },
				});
				const ctx = { url: running.url, agent };

				try {
					// Pin the protocol. Nothing in the dimensions is HTTP/1-specific, so
					// without this the row named controllable-h2 could run entirely over
					// HTTP/1.1 -- if allowHTTP1 were flipped, say -- and every assertion
					// would still pass. This is also what makes the H1 and H2 capabilities
					// load-bearing rather than decorative.
					const probe = await fetch(`${running.url}/hello`, {
						agent,
						timeout: 10_000,
					});
					await probe.text();
					t.equal(
						probe.version,
						server.expectVersion,
						`the row negotiates ${server.expectVersion}`,
					);

					// A dimension that lost its assertions would otherwise pass green:
					// tape treats a test that asserts nothing as a pass. Declaring the
					// count makes silent coverage loss -- and silent coverage gain --
					// fail. Counted from after the probe, so the declared number stays
					// about the dimension rather than about the runner.
					const before = t.assertCount;
					let expected = dimension.assertions;

					await dimension.run(t, ctx);
					if (dimension.negative && server.capabilities.has(C.SCRIPTABLE)) {
						await dimension.negative(t, ctx);
						expected += dimension.negativeAssertions;
					}

					t.equal(
						t.assertCount - before,
						expected,
						`ran its declared ${expected} assertions`,
					);
				} finally {
					// Deliberately no t.end() here, unlike the rest of the suite: tape
					// auto-ends an async test, and ending in the `finally` buries a
					// dimension's throw under a ".end() already called" failure plus a
					// spurious second one. In this harness the diagnosis is the product.
					await running.close();
				}
			},
		);
	}
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
