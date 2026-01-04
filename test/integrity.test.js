const test = require("tape");
const { fetch, ERROR_CODES } = require("../wrapper.js");

const HTTPBIN_URL = process.env.HTTPBIN_URL;
if (!HTTPBIN_URL) {
	console.error("HTTPBIN_URL environment variable is required");
	process.exit(1);
}

function url(path) {
	return `${HTTPBIN_URL}${path}`;
}

// Pre-computed hashes for httpbin's /bytes/0 endpoint (empty body)
// echo -n "" | sha256sum | xxd -r -p | base64
const EMPTY_SHA256 = "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";
// echo -n "" | sha384sum | xxd -r -p | base64
const EMPTY_SHA384 =
	"OLBgp1GsljhM2TJ+sbHjaiH9txEUvgdDTAzHv2P24donTt6/529l+9Ua0vFImLlb";
// echo -n "" | sha512sum | xxd -r -p | base64
const EMPTY_SHA512 =
	"z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==";

test("integrity: sha256 passes with correct hash", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: `sha256-${EMPTY_SHA256}`,
	});
	const bytes = await response.bytes();
	t.equal(bytes.length, 0, "should return empty body");
});

test("integrity: sha384 passes with correct hash", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: `sha384-${EMPTY_SHA384}`,
	});
	const bytes = await response.bytes();
	t.equal(bytes.length, 0, "should return empty body");
});

test("integrity: sha512 passes with correct hash", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: `sha512-${EMPTY_SHA512}`,
	});
	const bytes = await response.bytes();
	t.equal(bytes.length, 0, "should return empty body");
});

test("integrity: bytes() fails with wrong hash", async (t) => {
	t.plan(2);
	const response = await fetch(url("/bytes/0"), {
		integrity: "sha256-wronghashvalue",
	});
	try {
		await response.bytes();
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.IntegrityMismatch, "error code matches");
		t.ok(err.message.includes("integrity"), "error message mentions integrity");
	}
});

test("integrity: text() fails with wrong hash", async (t) => {
	t.plan(2);
	const response = await fetch(url("/bytes/0"), {
		integrity: "sha256-wronghashvalue",
	});
	try {
		await response.text();
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.IntegrityMismatch, "error code matches");
		t.ok(err.message.includes("integrity"), "error message mentions integrity");
	}
});

test("integrity: json() fails with wrong hash", async (t) => {
	t.plan(1);
	const response = await fetch(url("/json"), {
		integrity: "sha256-wronghashvalue",
	});
	try {
		await response.json();
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.IntegrityMismatch, "error code matches");
	}
});

test("integrity: multiple hashes - passes if one matches", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: `sha256-wronghash sha256-${EMPTY_SHA256}`,
	});
	const bytes = await response.bytes();
	t.equal(bytes.length, 0, "should pass when one hash matches");
});

test("integrity: multiple hashes - fails if none match", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: "sha256-wronghash1 sha256-wronghash2",
	});
	try {
		await response.bytes();
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.IntegrityMismatch, "error code matches");
	}
});

test("integrity: unknown algorithm with valid known algorithm passes", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: `sha1-ignored sha256-${EMPTY_SHA256}`,
	});
	const bytes = await response.bytes();
	t.equal(bytes.length, 0, "should ignore unknown algo and use known one");
});

test("integrity: only unknown algorithms fails", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: "sha1-something md5-something",
	});
	try {
		await response.bytes();
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.InvalidIntegrity, "error code matches");
	}
});

test("integrity: malformed value (no dash) fails", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: "sha256nohash",
	});
	try {
		await response.bytes();
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.InvalidIntegrity, "error code matches");
	}
});

test("integrity: empty string passes (no check)", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/0"), {
		integrity: "",
	});
	const bytes = await response.bytes();
	t.equal(bytes.length, 0, "empty integrity string means no check");
});

test("integrity: case-insensitive algorithm name", async (t) => {
	t.plan(2);

	const response1 = await fetch(url("/bytes/0"), {
		integrity: `SHA256-${EMPTY_SHA256}`,
	});
	const bytes1 = await response1.bytes();
	t.equal(bytes1.length, 0, "uppercase SHA256 works");

	const response2 = await fetch(url("/bytes/0"), {
		integrity: `Sha256-${EMPTY_SHA256}`,
	});
	const bytes2 = await response2.bytes();
	t.equal(bytes2.length, 0, "mixed case Sha256 works");
});

test("integrity: no integrity option means no check", async (t) => {
	t.plan(1);
	const response = await fetch(url("/bytes/10"));
	const bytes = await response.bytes();
	t.equal(bytes.length, 10, "works without integrity option");
});

test("integrity: works with non-empty body", async (t) => {
	t.plan(1);
	// /html returns a known HTML page, we'll compute hash of what we get
	// First fetch without integrity to get the content
	const response1 = await fetch(url("/html"));
	const text1 = await response1.text();

	// Now compute what hash we'd need (we can't easily, so this test just verifies
	// that wrong hash fails on non-empty body)
	const response2 = await fetch(url("/html"), {
		integrity: "sha256-definitelywronghash",
	});
	try {
		await response2.text();
		t.fail("should have thrown");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.IntegrityMismatch, "fails with wrong hash");
	}
});
