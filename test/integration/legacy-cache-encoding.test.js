/**
 * Content coding across the release that moved it into Faith (spec: ENC).
 *
 * Before that change the decode sat at the innermost layer of the HTTP stack, below the
 * cache: bodies were decoded whatever the request advertised, `Content-Encoding` and
 * `Content-Length` were always stripped, and the cache therefore stored decoded bodies with
 * no record of the coding they arrived in. Afterwards Faith owns the coding, decodes outside
 * the cache, and stores bodies as they came off the wire.
 *
 * Two things have to hold across that boundary, and neither can be checked from one build
 * alone -- so these run the previous release from npm alongside the working tree:
 *
 * 1. The wire does not change. The default `Accept-Encoding` the new build sends is
 *    byte-identical to what the old stack sent, so servers see the same request.
 * 2. A disk-cache entry written by the old release is still readable. Those entries hold
 *    already-decoded bodies with no `Content-Encoding`, so the new build finds no coding to
 *    decode and delivers them as-is. Nothing needs migrating.
 *
 * The previous release is installed into a temp directory on first run. With no network and
 * no cached install, these skip rather than fail: they are checking a released artefact, not
 * this working tree.
 */

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("tape");
const { spawn } = require("node:child_process");

const { fetch, Agent } = require("../../wrapper.js");
const { createEncodingOrigin, PAYLOAD } = require("../fixtures/encoding-origin.js");

/** The last release before Faith took over content coding. */
const LEGACY_VERSION = "0.4.0";
const LEGACY_DIR = path.join(os.tmpdir(), `faith-legacy-${LEGACY_VERSION}`);

/**
 * Run a command to completion, capturing its output.
 *
 * Async rather than `spawnSync` deliberately: the origin these tests fetch from lives in
 * this process, so blocking the event loop would leave the child's requests unanswered
 * until it had already timed out.
 */
function run(cmd, args, options = {}) {
	return new Promise((resolve) => {
		const child = spawn(cmd, args, { ...options, stdio: ["ignore", "pipe", "pipe"] });
		let stdout = "";
		let stderr = "";
		child.stdout.on("data", (chunk) => (stdout += chunk));
		child.stderr.on("data", (chunk) => (stderr += chunk));
		child.on("error", (err) => resolve({ code: -1, stdout, stderr: String(err) }));
		child.on("close", (code) => resolve({ code, stdout, stderr }));
	});
}

/** Install the previous release into `LEGACY_DIR`, reusing it once it is there. */
async function ensureLegacyFaith() {
	const entry = path.join(LEGACY_DIR, "node_modules", "@passcod", "faith", "index.js");
	if (fs.existsSync(entry)) return { ok: true };

	fs.mkdirSync(LEGACY_DIR, { recursive: true });
	fs.writeFileSync(
		path.join(LEGACY_DIR, "package.json"),
		JSON.stringify({ name: "faith-legacy-probe", version: "0.0.0", private: true }),
	);

	const install = await run(
		"npm",
		["install", "--no-audit", "--no-fund", `@passcod/faith@${LEGACY_VERSION}`],
		{ cwd: LEGACY_DIR },
	);
	if (install.code !== 0 || !fs.existsSync(entry)) {
		// The first real complaint, not npm's closing "a log of this run is at ..." line.
		const complaint = install.stderr
			.split("\n")
			.map((line) => line.replace(/^npm error\s*/, "").trim())
			.find((line) => line && !line.startsWith("A complete log"));
		return {
			ok: false,
			reason: `could not install @passcod/faith@${LEGACY_VERSION}: ${complaint || `exit ${install.code}`}`,
		};
	}
	return { ok: true };
}

/**
 * Run `body` inside the previous release, in its own process. `body` is source text, not a
 * closure: it is compiled by a different build of Faith than the one running these tests.
 */
async function inLegacyFaith(body) {
	const script = path.join(LEGACY_DIR, "probe.js");
	fs.writeFileSync(
		script,
		`const { fetch, Agent } = require("@passcod/faith");
		const report = (value) => console.log("__RESULT__" + JSON.stringify(value));
		(async () => {
			${body}
			process.exit(0);
		})().catch((err) => { console.error(String(err)); process.exit(1); });`,
	);

	const out = await run(process.execPath, [script], { cwd: LEGACY_DIR });
	const line = out.stdout.split("\n").find((l) => l.startsWith("__RESULT__"));
	if (!line) {
		throw new Error(
			`the ${LEGACY_VERSION} probe reported nothing (exit ${out.code}): ${out.stderr.trim()}`,
		);
	}
	return JSON.parse(line.slice("__RESULT__".length));
}

/** Spin up the origin, run `body`, tear everything down whatever happens. */
async function withOrigin(t, body) {
	const origin = createEncodingOrigin();
	const port = await origin.listen();
	const agents = [];
	try {
		await body({
			origin,
			port,
			agent: (options) => {
				const made = new Agent(options);
				agents.push(made);
				return made;
			},
		});
	} catch (err) {
		t.error(err, "the test body threw");
	} finally {
		for (const made of agents) made.close();
		await origin.close();
		t.end();
	}
}

test(`legacy ${LEGACY_VERSION}: the default Accept-Encoding is unchanged`, async (t) => {
	const ready = await ensureLegacyFaith();
	if (!ready.ok) {
		t.skip(ready.reason);
		t.end();
		return;
	}

	await withOrigin(t, async ({ origin, port }) => {
		const legacy = await inLegacyFaith(`
			const res = await fetch("http://127.0.0.1:${port}/echo");
			report({ advertised: (await res.json()).headers["accept-encoding"] });
		`);

		const res = await fetch(origin.url("/echo"), { timeout: 10000 });
		const current = (await res.json()).headers["accept-encoding"];

		t.equal(
			current,
			legacy.advertised,
			`byte-identical to what ${LEGACY_VERSION} sent, so servers negotiate the same way`,
		);
		t.equal(current, "zstd,gzip,deflate,br", "and it is the value both send");
	});
});

test(`legacy ${LEGACY_VERSION}: decoded whatever the request advertised`, async (t) => {
	const ready = await ensureLegacyFaith();
	if (!ready.ok) {
		t.skip(ready.reason);
		t.end();
		return;
	}

	// The defect the change fixed, pinned to the release that had it: this is what a caller
	// who set `Accept-Encoding: identity` used to get, and what they now no longer get.
	await withOrigin(t, async ({ origin, port }) => {
		const legacy = await inLegacyFaith(`
			const res = await fetch("http://127.0.0.1:${port}/coded/gzip", {
				headers: { "Accept-Encoding": "identity" },
			});
			const body = await res.bytes();
			report({
				contentEncoding: res.headers.get("content-encoding"),
				bodyLength: body.length,
				firstBytes: [body[0], body[1]],
			});
		`);

		t.equal(
			legacy.contentEncoding,
			null,
			`${LEGACY_VERSION} stripped Content-Encoding despite the request asking for identity`,
		);
		t.equal(
			legacy.bodyLength,
			PAYLOAD.length,
			"and handed over the decoded representation, the bytes as sent being unreachable",
		);

		// The same request against the working tree.
		const res = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "identity" },
			timeout: 10000,
		});
		const bytes = await res.bytes();
		t.equal(res.headers.get("content-encoding"), "gzip", "which now survives");
		t.deepEqual([bytes[0], bytes[1]], [0x1f, 0x8b], "over the gzip bytes as sent");
	});
});

test(`legacy ${LEGACY_VERSION}: a disk entry it wrote reads back as identity`, async (t) => {
	const ready = await ensureLegacyFaith();
	if (!ready.ok) {
		t.skip(ready.reason);
		t.end();
		return;
	}

	await withOrigin(t, async ({ origin, port, agent }) => {
		const store = fs.mkdtempSync(path.join(os.tmpdir(), "faith-legacy-cache-"));
		try {
			// `cacheable-novary` so the entry is matched on method and URL alone: whether the
			// two builds happen to send the same `Accept-Encoding` is the previous test's
			// business, not this one's.
			const url = `http://127.0.0.1:${port}/cacheable-novary/gzip`;
			const legacy = await inLegacyFaith(`
				const agent = new Agent({ cache: { store: "disk", path: ${JSON.stringify(store)} } });
				const res = await fetch(${JSON.stringify(url)}, { agent });
				const body = await res.text();
				report({
					contentEncoding: res.headers.get("content-encoding"),
					bodyLength: body.length,
					requestCount: res.headers.get("x-request-count"),
				});
				agent.close();
			`);

			t.equal(
				legacy.contentEncoding,
				null,
				`${LEGACY_VERSION} stored the body decoded, with no record of the coding`,
			);
			t.equal(legacy.bodyLength, PAYLOAD.length, "so the entry holds the representation");

			const countWhenStored = origin.count();
			const res = await fetch(url, {
				agent: agent({ cache: { store: "disk", path: store } }),
				timeout: 10000,
			});
			const text = await res.text();

			t.equal(
				origin.count(),
				countWhenStored,
				"the new build serves the old entry from disk, without going to the network",
			);
			t.equal(
				res.headers.get("content-encoding"),
				null,
				"the entry names no coding, so there is nothing to decode",
			);
			t.equal(text, PAYLOAD, "and it is delivered as-is: identity, needing no migration");
		} finally {
			fs.rmSync(store, { recursive: true, force: true });
		}
	});
});
