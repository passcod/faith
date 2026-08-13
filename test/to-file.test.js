const test = require("tape");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const crypto = require("node:crypto");
const { pathToFileURL } = require("node:url");
const { fetch, ERROR_CODES } = require("../wrapper.js");

const HTTPBIN_URL = process.env.HTTPBIN_URL;
if (!HTTPBIN_URL) {
	console.error("HTTPBIN_URL environment variable is required");
	process.exit(1);
}

function url(p) {
	return `${HTTPBIN_URL}${p}`;
}

// A deterministic, non-empty text body: two fetches return identical content.
const TEXT_PATH = "/robots.txt";

const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "faith-tofile-"));
let counter = 0;
function tmpPath(name = "out.bin") {
	return path.join(tmpRoot, `${counter++}-${name}`);
}
process.on("exit", () => {
	try {
		fs.rmSync(tmpRoot, { recursive: true, force: true });
	} catch {}
});

async function fetchBytes(p) {
	return await (await fetch(url(p))).bytes();
}

test("toFile: writes body and resolves { path, bytesWritten }", async (t) => {
	t.plan(4);
	const expected = await fetchBytes(TEXT_PATH);
	const dest = tmpPath("robots.txt");
	const res = await fetch(url(TEXT_PATH));
	const result = await res.toFile(dest);
	t.equal(
		fs.realpathSync.native(result.path),
		fs.realpathSync.native(dest),
		"resolves the absolute path written to",
	);
	t.equal(result.bytesWritten, expected.length, "bytesWritten counts the bytes");
	t.deepEqual(
		new Uint8Array(fs.readFileSync(dest)),
		new Uint8Array(expected),
		"file on disk holds exactly the body",
	);
	t.equal(res.bodyUsed, true, "bodyUsed is true after toFile");
});

test("toFile: relative path resolves against cwd and returns absolute", async (t) => {
	t.plan(3);
	const previous = process.cwd();
	process.chdir(tmpRoot);
	try {
		const res = await fetch(url(TEXT_PATH));
		const result = await res.toFile("relative-out.bin");
		t.ok(path.isAbsolute(result.path), "returned path is absolute");
		t.ok(
			fs.existsSync(path.resolve("relative-out.bin")),
			"file landed in the working directory",
		);
		// Compare the files rather than the spellings: Node and the native side can name
		// the same working directory differently (macOS /var against /private/var, Windows
		// short names against long ones), so canonicalise both before comparing.
		t.equal(
			fs.realpathSync.native(result.path),
			fs.realpathSync.native(path.resolve("relative-out.bin")),
			"the returned path names that file",
		);
	} finally {
		process.chdir(previous);
	}
});

test("toFile: writes to a file:// URL destination", async (t) => {
	t.plan(2);
	const expected = await fetchBytes(TEXT_PATH);
	const dest = tmpPath("via-url.bin");
	const res = await fetch(url(TEXT_PATH));
	const result = await res.toFile(pathToFileURL(dest));
	t.equal(
		fs.realpathSync.native(result.path),
		fs.realpathSync.native(dest),
		"resolves the file URL to its path",
	);
	t.deepEqual(
		new Uint8Array(fs.readFileSync(dest)),
		new Uint8Array(expected),
		"file holds the body",
	);
});

test("toFile: a present-but-empty body writes an empty file", async (t) => {
	t.plan(2);
	const dest = tmpPath("empty.bin");
	const res = await fetch(url("/bytes/0"));
	const result = await res.toFile(dest);
	t.equal(result.bytesWritten, 0, "no bytes written");
	t.equal(fs.readFileSync(dest).length, 0, "empty file on disk");
});

test("toFile: a decoded body is written decoded", async (t) => {
	t.plan(2);
	const dest = tmpPath("decoded.json");
	const res = await fetch(url("/gzip"));
	const result = await res.toFile(dest);
	const parsed = JSON.parse(fs.readFileSync(dest, "utf8"));
	t.ok(parsed.gzipped, "written body is the decoded JSON");
	t.equal(
		result.bytesWritten,
		fs.readFileSync(dest).length,
		"bytesWritten counts the decoded bytes on disk",
	);
});

test("toFile: sets bodyUsed and a second read is refused", async (t) => {
	t.plan(2);
	const res = await fetch(url(TEXT_PATH));
	await res.toFile(tmpPath());
	t.equal(res.bodyUsed, true, "bodyUsed is true");
	try {
		await res.bytes();
		t.fail("second read should be refused");
	} catch (err) {
		t.equal(
			err.code,
			ERROR_CODES.ResponseAlreadyDisturbed,
			"second read rejects already-disturbed",
		);
	}
});

test("toFile: after another read is refused and creates no file", async (t) => {
	t.plan(2);
	const res = await fetch(url(TEXT_PATH));
	await res.bytes();
	const dest = tmpPath("after-bytes.bin");
	try {
		await res.toFile(dest);
		t.fail("toFile should be refused");
	} catch (err) {
		t.equal(
			err.code,
			ERROR_CODES.ResponseAlreadyDisturbed,
			"rejects already-disturbed",
		);
	}
	t.notOk(fs.existsSync(dest), "no file created");
});

test("toFile: clone and original each write their own file", async (t) => {
	t.plan(2);
	const expected = await fetchBytes(TEXT_PATH);
	const res = await fetch(url(TEXT_PATH));
	const clone = res.clone();
	const d1 = tmpPath("orig.bin");
	const d2 = tmpPath("clone.bin");
	await res.toFile(d1);
	await clone.toFile(d2);
	t.deepEqual(
		new Uint8Array(fs.readFileSync(d1)),
		new Uint8Array(expected),
		"original wrote the body",
	);
	t.deepEqual(
		new Uint8Array(fs.readFileSync(d2)),
		new Uint8Array(expected),
		"clone wrote the body",
	);
});

test("toFile: webResponse() after toFile is refused", async (t) => {
	t.plan(1);
	const res = await fetch(url(TEXT_PATH));
	await res.toFile(tmpPath());
	try {
		res.webResponse();
		t.fail("webResponse should be refused");
	} catch (err) {
		t.equal(
			err.code,
			ERROR_CODES.ResponseAlreadyDisturbed,
			"webResponse rejects already-disturbed",
		);
	}
});

test("toFile: discard() after toFile is accepted", async (t) => {
	t.plan(1);
	const res = await fetch(url(TEXT_PATH));
	await res.toFile(tmpPath());
	await res.discard();
	t.pass("discard after toFile resolves");
});

test("toFile: default refuses an occupied destination with FileExists", async (t) => {
	t.plan(2);
	const dest = tmpPath("occupied.bin");
	fs.writeFileSync(dest, "original contents");
	const res = await fetch(url(TEXT_PATH));
	try {
		await res.toFile(dest);
		t.fail("should refuse an occupied destination");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.FileExists, "rejects FileExists");
	}
	t.equal(
		fs.readFileSync(dest, "utf8"),
		"original contents",
		"existing file left untouched",
	);
});

test("toFile: overwrite truncates and replaces an existing file", async (t) => {
	t.plan(2);
	const expected = await fetchBytes(TEXT_PATH);
	const dest = tmpPath("replace.bin");
	fs.writeFileSync(dest, "x".repeat(expected.length + 50));
	const res = await fetch(url(TEXT_PATH));
	const result = await res.toFile(dest, { overwrite: true });
	t.equal(result.bytesWritten, expected.length, "wrote the new body");
	t.deepEqual(
		new Uint8Array(fs.readFileSync(dest)),
		new Uint8Array(expected),
		"file replaced with the body, no leftover tail",
	);
});

test("toFile: a missing parent directory fails with FileWrite", async (t) => {
	t.plan(1);
	const dest = path.join(tmpRoot, "no-such-dir", "out.bin");
	const res = await fetch(url(TEXT_PATH));
	try {
		await res.toFile(dest);
		t.fail("should fail on a missing parent directory");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.FileWrite, "rejects FileWrite");
	}
});

test("toFile: mode sets the permissions of a new file", async (t) => {
	if (process.platform === "win32") {
		t.pass("skipped on Windows");
		t.end();
		return;
	}
	t.plan(1);
	const dest = tmpPath("moded.bin");
	const res = await fetch(url(TEXT_PATH));
	await res.toFile(dest, { mode: 0o600 });
	const expectedMode = 0o600 & ~process.umask();
	t.equal(
		fs.statSync(dest).mode & 0o777,
		expectedMode,
		"new file carries the requested mode",
	);
});

test("toFile: a file:// URL that is not a local path throws InvalidPath at the call", async (t) => {
	t.plan(3);
	const res = await fetch(url(TEXT_PATH));

	// A host names another machine, so the URL is not a local path. Windows' own conversion
	// would turn it into a UNC path rather than refusing it, which the wrapper catches first.
	let thrown;
	let pending;
	try {
		pending = res.toFile(new URL("file://example.com/tmp/out.bin"));
	} catch (err) {
		thrown = err;
	}
	// Should the call ever stop throwing, swallow the rejection so this fails the assertion
	// below rather than killing the run with an unhandled rejection.
	pending?.catch(() => {});

	t.equal(
		thrown?.code,
		ERROR_CODES.InvalidPath,
		"throws InvalidPath synchronously",
	);
	t.equal(res.bodyUsed, false, "body is untouched");
	const bytes = await res.bytes();
	t.ok(bytes.length > 0, "body is still readable afterwards");
});

test("toFile: on a bodyless response throws ResponseBodyNull and creates no file", async (t) => {
	t.plan(2);
	const dest = tmpPath("bodyless.bin");
	const res = await fetch(url("/status/204"));
	try {
		await res.toFile(dest);
		t.fail("should refuse a bodyless response");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.ResponseBodyNull, "rejects ResponseBodyNull");
	}
	t.notOk(fs.existsSync(dest), "no file created");
});

test("toFile: integrity match passes and writes the file", async (t) => {
	t.plan(2);
	const expected = await fetchBytes(TEXT_PATH);
	const digest = crypto.createHash("sha256").update(expected).digest("base64");
	const dest = tmpPath("integrity-ok.bin");
	const res = await fetch(url(TEXT_PATH), { integrity: `sha256-${digest}` });
	const result = await res.toFile(dest);
	t.equal(result.bytesWritten, expected.length, "wrote the body");
	t.deepEqual(
		new Uint8Array(fs.readFileSync(dest)),
		new Uint8Array(expected),
		"file holds the verified body",
	);
});

test("toFile: integrity mismatch throws and leaves the file on disk", async (t) => {
	t.plan(2);
	const dest = tmpPath("integrity-bad.bin");
	const res = await fetch(url(TEXT_PATH), {
		integrity: "sha256-definitelythewronghashvalueforthisbody",
	});
	try {
		await res.toFile(dest);
		t.fail("should fail integrity verification");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.IntegrityMismatch, "rejects IntegrityMismatch");
	}
	t.ok(fs.existsSync(dest), "the file that failed verification is on disk");
});

test("toFile: writes a body within the advertised Content-Length", async (t) => {
	t.plan(1);
	const dest = tmpPath("sized.bin");
	const res = await fetch(url("/bytes/2048"));
	const result = await res.toFile(dest);
	t.equal(result.bytesWritten, 2048, "wrote the advertised number of bytes");
});

test("toFile: onProgress reports the bytes as they land", async (t) => {
	t.plan(4);
	const dest = tmpPath("progress.bin");
	const reports = [];
	const res = await fetch(url(TEXT_PATH));
	const result = await res.toFile(dest, {
		onProgress: (progress) => reports.push(progress),
	});

	t.ok(reports.length >= 1, "reports at least once");
	const last = reports[reports.length - 1];
	t.equal(
		last.bytesWritten,
		result.bytesWritten,
		"the final report totals the whole body",
	);
	t.equal(
		last.contentLength,
		result.bytesWritten,
		"and carries the advertised total",
	);
	t.ok(
		reports.every((r) => r.bytesWritten <= result.bytesWritten),
		"no report overcounts",
	);
});

test("toFile: onProgress reports repeatedly across a slow body", async (t) => {
	t.plan(3);
	const dest = tmpPath("progress-drip.bin");
	const reports = [];
	// Dripped over well beyond the reporting interval, so the write spans several of them.
	const res = await fetch(url("/drip?duration=0.6&numbytes=120&delay=0"));
	const result = await res.toFile(dest, {
		onProgress: (progress) => reports.push(progress),
	});

	t.equal(result.bytesWritten, 120, "wrote the whole body");
	t.ok(reports.length > 1, `reports more than once (${reports.length} reports)`);
	const counts = reports.map((r) => r.bytesWritten);
	t.deepEqual(
		counts,
		[...counts].sort((a, b) => a - b),
		"the counts only ever climb",
	);
});

test("toFile: onProgress reports no total for a decoded body", async (t) => {
	t.plan(2);
	const dest = tmpPath("progress-decoded.json");
	const reports = [];
	// Faith decodes this, so the wire length says nothing about the size on disk.
	const res = await fetch(url("/gzip"));
	const result = await res.toFile(dest, {
		onProgress: (progress) => reports.push(progress),
	});

	t.ok(reports.length >= 1, "reports at least once");
	t.equal(
		reports[reports.length - 1].contentLength,
		undefined,
		"no advertised total for a body Faith decodes",
	);
	void result;
});

test("toFile: onProgress reports once for an empty body", async (t) => {
	t.plan(2);
	const dest = tmpPath("progress-empty.bin");
	const reports = [];
	const res = await fetch(url("/bytes/0"));
	await res.toFile(dest, { onProgress: (progress) => reports.push(progress) });

	t.equal(reports.length, 1, "a write with nothing to report still reports once");
	t.equal(reports[0].bytesWritten, 0, "having written nothing");
});

test("toFile: a non-function onProgress is refused", async (t) => {
	t.plan(2);
	const res = await fetch(url(TEXT_PATH));
	t.throws(
		() => res.toFile(tmpPath(), { onProgress: "not a function" }),
		TypeError,
		"throws a TypeError",
	);
	t.equal(res.bodyUsed, false, "body is untouched");
});

test("ERROR_CODES exposes the toFile error codes", async (t) => {
	t.plan(5);
	t.equal(ERROR_CODES.ContentLengthOverrun, "ContentLengthOverrun");
	t.equal(ERROR_CODES.FileExists, "FileExists");
	t.equal(ERROR_CODES.FileWrite, "FileWrite");
	t.equal(ERROR_CODES.InvalidPath, "InvalidPath");
	t.equal(ERROR_CODES.ResponseBodyNull, "ResponseBodyNull");
});
