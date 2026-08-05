/**
 * The file tree the configured servers serve.
 *
 * Caddy, nginx and Apache are configured, not scripted: their contribution to the
 * matrix is what *their own* compression, ETag and framing code does to a file on
 * disk. So each of them serves this tree, and the contract's paths are laid out as
 * real files under it.
 *
 * Only the contract's static routes appear here. Trailers have no static
 * equivalent -- a file cannot carry one -- which is why those rows do not declare
 * TRAILERS and the trailers dimension skips them.
 */

const { mkdtempSync, mkdirSync, rmSync, writeFileSync, utimesSync } = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const { PAYLOAD, COMPRESSIBLE } = require("../contract.js");

/**
 * Fixed mtime, so Last-Modified and any ETag derived from it are the same on every
 * run. These servers build ETags from mtime and size, and a validator that changed
 * per run would make a conditional-request failure look like a client bug when it
 * was really the fixture moving underneath the assertion.
 */
const MTIME = new Date("2020-01-01T00:00:00Z");

/**
 * Contract paths and what goes in each file.
 *
 * The encoding routes get the large body: every one of these servers refuses to
 * compress something PAYLOAD's size, and would then serve plain bytes that the
 * gzip dimension cannot tell from a compressed round-trip.
 */
const FILES = [
	["hello", PAYLOAD],
	["framing/length", PAYLOAD],
	["conditional/etag", PAYLOAD],
	["encoding/gzip", COMPRESSIBLE],
	["encoding/mislabelled", COMPRESSIBLE],
];

function buildStaticTree() {
	const dir = mkdtempSync(path.join(os.tmpdir(), "faith-conformance-"));
	for (const [rel, body] of FILES) {
		const target = path.join(dir, rel);
		mkdirSync(path.dirname(target), { recursive: true });
		writeFileSync(target, body);
		utimesSync(target, MTIME, MTIME);
	}
	return {
		dir,
		cleanup: () => rmSync(dir, { recursive: true, force: true }),
	};
}

module.exports = { buildStaticTree, MTIME };
