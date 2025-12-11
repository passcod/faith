const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Compression - gzip encoding should be handled automatically", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/gzip"));
	t.ok(response.ok, "Should successfully fetch gzip-compressed response");
	t.equal(response.status, 200, "Status should be 200");

	const data = await response.json();
	t.ok(data.gzipped, "Response should indicate it was gzipped");
});

test("Compression - deflate encoding should be handled automatically", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/deflate"));
	t.ok(response.ok, "Should successfully fetch deflate-compressed response");
	t.equal(response.status, 200, "Status should be 200");

	const data = await response.json();
	t.ok(data.deflated, "Response should indicate it was deflated");
});

test("Compression - brotli encoding should be handled automatically", async (t) => {
	const response = await faithFetch(url("/brotli"));

	// go-httpbin doesn't support brotli (returns 501)
	if (response.status === 501) {
		t.pass("Skipping brotli test - not supported by server");
		t.end();
		return;
	}

	t.plan(3);
	t.ok(response.ok, "Should successfully fetch brotli-compressed response");
	t.equal(response.status, 200, "Status should be 200");

	const data = await response.json();
	t.ok(data.brotli, "Response should indicate it was brotli-compressed");
});

test("Compression - gzip with custom headers", async (t) => {
	t.plan(4);

	const response = await faithFetch(url("/gzip"), {
		headers: {
			"Accept-Encoding": "gzip",
		},
	});
	t.ok(response.ok, "Should successfully fetch with explicit gzip header");
	t.equal(response.status, 200, "Status should be 200");

	const data = await response.json();
	t.ok(data.gzipped, "Response should indicate it was gzipped");
	t.ok(data.headers, "Response should include headers");
});

test("Compression - deflate with custom headers", async (t) => {
	t.plan(4);

	const response = await faithFetch(url("/deflate"), {
		headers: {
			"Accept-Encoding": "deflate",
		},
	});
	t.ok(response.ok, "Should successfully fetch with explicit deflate header");
	t.equal(response.status, 200, "Status should be 200");

	const data = await response.json();
	t.ok(data.deflated, "Response should indicate it was deflated");
	t.ok(data.headers, "Response should include headers");
});

test("Compression - large gzipped response", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/stream-bytes/10000"));
	t.ok(response.ok, "Should successfully fetch large response");
	t.equal(response.status, 200, "Status should be 200");

	const buffer = await response.arrayBuffer();
	t.ok(buffer.byteLength > 0, "Should receive decompressed data");
});

test("Compression - multiple encodings in sequence", async (t) => {
	t.plan(4);

	const endpoints = ["/gzip", "/deflate"];

	for (const endpoint of endpoints) {
		const response = await faithFetch(url(endpoint));
		t.ok(response.ok, `Should successfully fetch ${endpoint}`);
		const data = await response.json();
		t.ok(data, `Should parse JSON from ${endpoint}`);
	}
});

test("Compression - no compression with identity encoding", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/get"), {
		headers: {
			"Accept-Encoding": "identity",
		},
	});
	t.ok(response.ok, "Should successfully fetch with identity encoding");
	t.equal(response.status, 200, "Status should be 200");

	const data = await response.json();
	t.ok(data, "Should parse JSON response");
});
