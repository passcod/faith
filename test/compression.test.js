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

test("Compression - default request advertises the four codings", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/get"));
	const data = await response.json();
	// go-httpbin returns request headers as arrays.
	t.deepEqual(
		data.headers["Accept-Encoding"],
		["zstd,gzip,deflate,br"],
		"Default Accept-Encoding should match the value the stack sent before",
	);
});

test("Compression - identity delivers the compressed bytes as sent", async (t) => {
	t.plan(5);

	// go-httpbin's /gzip compresses whatever the request advertised.
	const response = await faithFetch(url("/gzip"), {
		headers: {
			"Accept-Encoding": "identity",
		},
	});
	t.ok(response.ok, "Should successfully fetch");
	t.equal(
		response.headers.get("content-encoding"),
		"gzip",
		"Content-Encoding should survive undecoded",
	);
	t.ok(
		response.headers.get("content-length"),
		"Content-Length should survive undecoded",
	);

	const buffer = await response.bytes();
	t.equal(buffer[0], 0x1f, "First byte should be the gzip magic 0x1f");
	t.equal(buffer[1], 0x8b, "Second byte should be the gzip magic 0x8b");
});

test("Compression - a coding the request did not accept is delivered as received", async (t) => {
	t.plan(2);

	// /gzip sends gzip, but the request accepts only brotli.
	const response = await faithFetch(url("/gzip"), {
		headers: {
			"Accept-Encoding": "br",
		},
	});
	t.equal(
		response.headers.get("content-encoding"),
		"gzip",
		"Content-Encoding should survive undecoded",
	);
	const buffer = await response.bytes();
	t.equal(buffer[0], 0x1f, "Body should be the raw gzip bytes");
});

test("Compression - a coding named outright settles it over a wildcard", async (t) => {
	t.plan(2);

	// `gzip;q=0, *` refuses gzip while accepting everything else.
	const response = await faithFetch(url("/gzip"), {
		headers: {
			"Accept-Encoding": "gzip;q=0, *",
		},
	});
	t.equal(
		response.headers.get("content-encoding"),
		"gzip",
		"gzip refused, so the body is delivered as received",
	);
	const buffer = await response.bytes();
	t.equal(buffer[0], 0x1f, "Body should be the raw gzip bytes");
});

test("Compression - default decoding strips the coding headers", async (t) => {
	t.plan(3);

	const response = await faithFetch(url("/gzip"));
	t.equal(
		response.headers.get("content-encoding"),
		null,
		"Content-Encoding is removed on decoding",
	);
	t.equal(
		response.headers.get("content-length"),
		null,
		"Content-Length is removed on decoding",
	);
	const data = await response.json();
	t.ok(data.gzipped, "Body is decoded");
});

test("Compression - the body stream is decoded too", async (t) => {
	t.plan(1);

	const response = await faithFetch(url("/gzip"));
	const reader = response.body.getReader();
	const chunks = [];
	while (true) {
		const { done, value } = await reader.read();
		if (done) break;
		chunks.push(Buffer.from(value));
	}
	const data = JSON.parse(Buffer.concat(chunks).toString("utf8"));
	t.ok(data.gzipped, "Stream delivers decoded bytes");
});
