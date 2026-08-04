/**
 * Computes and runs the conformance matrix.
 *
 * The matrix is derived, not written down: each server declares its
 * capabilities, each dimension declares its requirements, and a cell runs only
 * when the requirements are met. A hand-maintained matrix would drift from what
 * the servers can actually do the first time a build changed.
 *
 * Emits `matrix.json` alongside the human-readable output, so the realised
 * matrix can later be rendered into the README without re-deriving it.
 */

const test = require("tape");
const { writeFileSync } = require("node:fs");
const path = require("node:path");

const { CAPABILITIES: C, assertKnownCapabilities } = require("./capabilities.js");
const { controllableH1, controllableH2 } = require("./servers/controllable.js");

const { Agent } = require("../../index.js");

const SERVERS = [controllableH1, controllableH2];
const DIMENSIONS = [require("./dimensions/trailers.js")];

const EXPECTED = require("./expected-matrix.json");

function planCells() {
	const cells = [];
	for (const server of SERVERS) {
		assertKnownCapabilities(server.capabilities, `server ${server.name}`);
		for (const dimension of DIMENSIONS) {
			assertKnownCapabilities(dimension.requires, `dimension ${dimension.name}`);
			const missing = dimension.requires.filter((c) => !server.capabilities.has(c));
			cells.push({
				server: server.name,
				dimension: dimension.name,
				status: missing.length === 0 ? "run" : "skip",
				reason: missing.length === 0 ? null : `lacks ${missing.join(", ")}`,
			});
		}
	}
	return cells;
}

async function main() {
	const cells = planCells();

	// Structured output first, so it exists even if a cell later fails.
	const out = path.join(__dirname, "matrix.json");
	writeFileSync(out, `${JSON.stringify({ cells }, null, "\t")}\n`);

	test("conformance: realised matrix matches the expected one", (t) => {
		// A cell silently disappearing -- because a capability declaration
		// changed, or a server stopped starting -- looks identical to a clean run
		// unless the shape itself is asserted.
		t.deepEqual(
			cells.map(({ server, dimension, status }) => ({ server, dimension, status })),
			EXPECTED.cells,
			"no cell appeared or vanished unnoticed",
		);
		t.end();
	});

	for (const cell of cells) {
		if (cell.status === "skip") {
			test(`${cell.server} / ${cell.dimension}`, (t) => {
				t.pass(`skipped: ${cell.reason}`);
				t.end();
			});
			continue;
		}

		const server = SERVERS.find((s) => s.name === cell.server);
		const dimension = DIMENSIONS.find((d) => d.name === cell.dimension);

		test(`${cell.server} / ${cell.dimension}`, async (t) => {
			let running;
			try {
				running = await server.start();
			} catch (err) {
				// Loud, not skipped: a server that will not start is a failure of
				// the row, and its own log is the only useful diagnostic.
				t.fail(`${cell.server} failed to start: ${err.message}`);
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
				await dimension.run(t, ctx);
				if (dimension.negative && server.capabilities.has(C.SCRIPTABLE)) {
					await dimension.negative(t, ctx);
				}
			} finally {
				await running.close();
				t.end();
			}
		});
	}
}

main();
