/**
 * Content coding: which responses Faith decodes, and which it hands over as sent.
 *
 * The decode decision rests on the `Accept-Encoding` the request carried, not on a static
 * client setting (spec: ENC). So a caller who advertises `identity` gets the bytes as sent
 * with `Content-Encoding` and `Content-Length` intact, and a caller who advertises nothing
 * gets the decoded body Faith negotiated for.
 *
 * These use an origin of ours rather than go-httpbin, which compresses on its own terms and
 * has no route for a layered coding, a cacheable coded response, or a `HEAD` describing a
 * compressed representation. See `test/fixtures/encoding-origin.js`.
 */

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("tape");
const zlib = require("node:zlib");

const { fetch, Agent } = require("../wrapper.js");
const { createEncodingOrigin, PAYLOAD } = require("./fixtures/encoding-origin.js");

/** Spin up the origin, run `body`, and tear it down whatever happens. */
async function withOrigin(t, body) {
	const origin = createEncodingOrigin();
	await origin.listen();
	const agents = [];
	const agent = (options) => {
		const made = new Agent(options);
		agents.push(made);
		return made;
	};
	try {
		await body({ origin, agent });
	} catch (err) {
		// Reported here rather than left to propagate past `t.end()`, which tape renders as
		// a pair of confusing ".end() already called" failures instead of the actual error.
		t.error(err, "the test body threw");
	} finally {
		for (const made of agents) made.close();
		await origin.close();
		t.end();
	}
}

const GZIP_MAGIC = [0x1f, 0x8b];

test("encoding: the default request advertises the four codings Faith decodes", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/echo"), { timeout: 10000 });
		const seen = await res.json();
		t.equal(
			seen.headers["accept-encoding"],
			"zstd,gzip,deflate,br",
			"the value the stack sent before Faith took over the codings",
		);
	});
});

for (const coding of ["gzip", "deflate", "br", "zstd"]) {
	test(`encoding: a default request decodes ${coding}`, async (t) => {
		await withOrigin(t, async ({ origin }) => {
			const res = await fetch(origin.url(`/coded/${coding}`), { timeout: 10000 });
			if (res.status === 501) {
				t.skip(`this Node cannot produce ${coding}`);
				return;
			}
			t.equal(
				res.headers.get("content-encoding"),
				null,
				"Content-Encoding is removed on decoding",
			);
			t.equal(
				res.headers.get("content-length"),
				null,
				"Content-Length is removed on decoding, having described the encoded bytes",
			);
			t.equal(await res.text(), PAYLOAD, "and the body reads back decoded");
		});
	});
}

test("encoding: identity hands over the bytes as sent", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "identity" },
			timeout: 10000,
		});

		t.equal(res.headers.get("content-encoding"), "gzip", "Content-Encoding survives");
		const encodedLength = Number(res.headers.get("content-length"));
		t.ok(encodedLength > 0, "Content-Length survives");

		const bytes = await res.bytes();
		t.deepEqual(
			[bytes[0], bytes[1]],
			GZIP_MAGIC,
			"the body is the gzip stream, not what it decodes to",
		);
		t.equal(bytes.length, encodedLength, "and Content-Length describes those bytes");
		t.equal(
			zlib.gunzipSync(bytes).toString("utf8"),
			PAYLOAD,
			"which the caller can decode themselves",
		);
	});
});

test("encoding: identity is what goes out on the wire", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "identity" },
			timeout: 10000,
		});
		t.equal(
			origin.requests().at(-1).acceptEncoding,
			"identity",
			"sent as given, not replaced by the default",
		);
	});
});

test("encoding: a coding the request did not accept is delivered as received", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// The origin sends gzip; the request accepts only brotli.
		const res = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "br" },
			timeout: 10000,
		});
		t.equal(res.headers.get("content-encoding"), "gzip", "Content-Encoding survives");
		const bytes = await res.bytes();
		t.deepEqual([bytes[0], bytes[1]], GZIP_MAGIC, "and the gzip bytes come through");
	});
});

test("encoding: a coding named outright settles it over a wildcard", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// `gzip;q=0, *` refuses gzip while accepting the other three.
		const refused = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "gzip;q=0, *" },
			timeout: 10000,
		});
		t.equal(refused.headers.get("content-encoding"), "gzip", "gzip is refused");
		const bytes = await refused.bytes();
		t.deepEqual([bytes[0], bytes[1]], GZIP_MAGIC, "so its bytes come through as sent");

		const accepted = await fetch(origin.url("/coded/br"), {
			headers: { "Accept-Encoding": "gzip;q=0, *" },
			timeout: 10000,
		});
		t.equal(
			accepted.headers.get("content-encoding"),
			null,
			"while the wildcard still covers brotli",
		);
		t.equal(await accepted.text(), PAYLOAD, "which is decoded");
	});
});

test("encoding: a coding Faith cannot decode is delivered as received", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// `Content-Encoding: compress` over bytes that are not actually compressed. Faith has
		// no decoder for it, so the bytes must come through untouched -- which is exactly why
		// the body reads back as the payload. Decoding anything here would error instead.
		const res = await fetch(origin.url("/mislabelled/compress"), { timeout: 10000 });

		t.equal(res.headers.get("content-encoding"), "compress", "Content-Encoding survives");
		t.ok(res.headers.get("content-length"), "and Content-Length survives with it");
		t.equal(await res.text(), PAYLOAD, "and the bytes come through untouched");
	});
});

test("encoding: a HEAD response keeps the coding headers", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/coded/gzip"), { method: "HEAD", timeout: 10000 });

		t.equal(
			res.headers.get("content-encoding"),
			"gzip",
			"nothing was decoded, so Content-Encoding stays",
		);
		// The same GET is decoded and loses both headers, so this is the HEAD keeping what a
		// GET gives up -- a HEAD has to keep describing the representation a GET would return.
		const encodedLength = Number(res.headers.get("content-length"));
		t.ok(encodedLength > 0, "and Content-Length stays, sized to the encoded representation");

		const decoded = await fetch(origin.url("/coded/gzip"), { timeout: 10000 });
		const decodedLength = (await decoded.bytes()).length;
		t.ok(
			encodedLength < decodedLength,
			`sized to the gzip stream (${encodedLength}) not the representation (${decodedLength})`,
		);
		t.equal(res.body, null, "with no body of its own");
	});
});

test("encoding: a bodyless response the request refused the coding of keeps them too", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// Both reasons not to decode at once: nothing to decode, and a coding the request
		// refused. Neither strips the headers.
		const res = await fetch(origin.url("/coded/zstd"), {
			method: "HEAD",
			headers: { "Accept-Encoding": "identity" },
			timeout: 10000,
		});
		if (res.status === 501) {
			t.skip("this Node cannot produce zstd");
			return;
		}
		t.equal(res.headers.get("content-encoding"), "zstd", "Content-Encoding stays");
		t.ok(Number(res.headers.get("content-length")) > 0, "and so does Content-Length");
	});
});

test("encoding: an agent default Accept-Encoding governs decoding", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		const identity = agent({
			headers: [{ name: "Accept-Encoding", value: "identity" }],
		});

		const echo = await fetch(origin.url("/echo"), { agent: identity, timeout: 10000 });
		const seen = await echo.json();
		t.equal(
			seen.headers["accept-encoding"],
			"identity",
			"the agent default is what goes out on the wire",
		);

		const res = await fetch(origin.url("/coded/gzip"), { agent: identity, timeout: 10000 });
		t.equal(
			res.headers.get("content-encoding"),
			"gzip",
			"and it decides the decode as a request header would",
		);
		const bytes = await res.bytes();
		t.deepEqual([bytes[0], bytes[1]], GZIP_MAGIC, "so the gzip bytes come through");
	});
});

test("encoding: a request header beats the agent default", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		const identity = agent({
			headers: [{ name: "Accept-Encoding", value: "identity" }],
		});

		const res = await fetch(origin.url("/coded/gzip"), {
			agent: identity,
			headers: { "Accept-Encoding": "gzip" },
			timeout: 10000,
		});
		t.equal(
			origin.requests().at(-1).acceptEncoding,
			"gzip",
			"the request's value replaces the agent's",
		);
		t.equal(res.headers.get("content-encoding"), null, "and governs the decode with it");
		t.equal(await res.text(), PAYLOAD, "so the body is decoded");
	});
});

test("encoding: an agent whose defaults do not name Accept-Encoding still sends the default", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		const other = agent({ headers: [{ name: "X-Marker", value: "set" }] });
		const res = await fetch(origin.url("/echo"), { agent: other, timeout: 10000 });
		const seen = await res.json();
		t.equal(
			seen.headers["accept-encoding"],
			"zstd,gzip,deflate,br",
			"other default headers do not displace the Accept-Encoding Faith sends",
		);
		t.equal(seen.headers["x-marker"], "set", "and the agent's own default still goes out");
	});
});

test("encoding: layered codings on one header line are delivered as received", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// gzip applied first, then brotli over the top: `Content-Encoding: gzip, br`.
		const res = await fetch(origin.url("/layered/gzip/br"), { timeout: 10000 });
		if (res.status === 501) {
			t.skip("this Node cannot produce one of the codings");
			return;
		}

		t.equal(
			res.headers.get("content-encoding"),
			"gzip, br",
			"Faith decodes a single coding, so both survive on the header",
		);
		t.ok(res.headers.get("content-length"), "and Content-Length survives with them");

		const bytes = await res.bytes();
		t.notDeepEqual(
			[bytes[0], bytes[1]],
			GZIP_MAGIC,
			"the body is the outermost coding, brotli, not the gzip beneath it",
		);
		// The caller unwinds it, outermost first.
		const once = zlib.brotliDecompressSync(bytes);
		t.deepEqual([once[0], once[1]], GZIP_MAGIC, "under which is the gzip stream");
		t.equal(
			zlib.gunzipSync(once).toString("utf8"),
			PAYLOAD,
			"and under that the representation",
		);
	});
});

test("encoding: layered codings split across header lines are delivered as received", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// One `Content-Encoding` line per coding: the same list as the comma-joined form, so
		// neither coding is decoded. Reading only the first line here would decode gzip and
		// hand back bytes that are still brotli underneath.
		const res = await fetch(origin.url("/layered-lines/gzip/br"), { timeout: 10000 });
		if (res.status === 501) {
			t.skip("this Node cannot produce one of the codings");
			return;
		}

		t.ok(
			res.headers.get("content-encoding").includes("gzip") &&
				res.headers.get("content-encoding").includes("br"),
			"both codings survive on the header",
		);

		const bytes = await res.bytes();
		t.equal(
			zlib.gunzipSync(zlib.brotliDecompressSync(bytes)).toString("utf8"),
			PAYLOAD,
			"and the body is doubly encoded, for the caller to unwind",
		);
	});
});

test("encoding: the body stream is decoded like the whole-body methods", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/coded/gzip"), { timeout: 10000 });
		const reader = res.body.getReader();
		const chunks = [];
		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			chunks.push(Buffer.from(value));
		}
		t.equal(
			Buffer.concat(chunks).toString("utf8"),
			PAYLOAD,
			"the stream delivers decoded bytes",
		);
	});
});

test("encoding: the body stream of an undecoded response is the bytes as sent", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "identity" },
			timeout: 10000,
		});
		const reader = res.body.getReader();
		const chunks = [];
		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			chunks.push(Buffer.from(value));
		}
		const bytes = Buffer.concat(chunks);
		t.deepEqual([bytes[0], bytes[1]], GZIP_MAGIC, "the stream carries the gzip stream");
		t.equal(
			zlib.gunzipSync(bytes).toString("utf8"),
			PAYLOAD,
			"which decodes to the representation",
		);
	});
});

test("encoding: integrity is checked over the bytes the caller receives", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// An undecoded response hands over encoded bytes, so the digest is over those.
		const probe = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "identity" },
			timeout: 10000,
		});
		const encoded = Buffer.from(await probe.bytes());
		const digestOfEncoded = require("node:crypto")
			.createHash("sha256")
			.update(encoded)
			.digest("base64");
		const digestOfDecoded = require("node:crypto")
			.createHash("sha256")
			.update(zlib.gunzipSync(encoded))
			.digest("base64");

		const matching = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "identity" },
			integrity: `sha256-${digestOfEncoded}`,
			timeout: 10000,
		});
		const bytes = await matching.bytes();
		t.deepEqual(
			[bytes[0], bytes[1]],
			GZIP_MAGIC,
			"a digest over the encoded bytes passes on an undecoded response",
		);

		const mismatching = await fetch(origin.url("/coded/gzip"), {
			headers: { "Accept-Encoding": "identity" },
			integrity: `sha256-${digestOfDecoded}`,
			timeout: 10000,
		});
		try {
			await mismatching.bytes();
			t.fail("a digest over the decoded bytes should not pass");
		} catch (err) {
			t.equal(err.code, "IntegrityMismatch", "it fails as an integrity mismatch");
		}

		// And on a decoded response the digest is over the decoded bytes.
		const decodedDigest = await fetch(origin.url("/coded/gzip"), {
			integrity: `sha256-${digestOfDecoded}`,
			timeout: 10000,
		});
		t.equal(
			await decodedDigest.text(),
			PAYLOAD,
			"a digest over the decoded bytes passes on a decoded response",
		);
	});
});

test("encoding: a cached response is stored as received and decoded on the way out", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		const cached = agent({ cache: { store: "memory" } });

		const first = await fetch(origin.url("/cacheable/gzip"), {
			agent: cached,
			timeout: 10000,
		});
		t.equal(await first.text(), PAYLOAD, "the first response is decoded");
		const firstCount = Number(first.headers.get("x-request-count"));

		const second = await fetch(origin.url("/cacheable/gzip"), {
			agent: cached,
			timeout: 10000,
		});
		t.equal(
			Number(second.headers.get("x-request-count")),
			firstCount,
			"the second is served from the cache, the origin not having seen it",
		);
		t.equal(
			second.headers.get("content-encoding"),
			null,
			"Content-Encoding is stripped on the way out, as on a network response",
		);
		t.equal(await second.text(), PAYLOAD, "and the cached body is decoded too");
	});
});

test("encoding: one stored entry answers callers who negotiated different codings", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		// No `Vary` here, so the entry stored by the first request is the one served to the
		// second whatever it advertised. That is the point of storing bytes as received: the
		// coding is decided per request on the way out, not fixed when the entry was written.
		const cached = agent({ cache: { store: "memory" } });

		const negotiated = await fetch(origin.url("/cacheable-novary/gzip"), {
			agent: cached,
			timeout: 10000,
		});
		t.equal(await negotiated.text(), PAYLOAD, "the storing request gets a decoded body");
		const storedFrom = Number(negotiated.headers.get("x-request-count"));

		const asSent = await fetch(origin.url("/cacheable-novary/gzip"), {
			agent: cached,
			headers: { "Accept-Encoding": "identity" },
			timeout: 10000,
		});
		t.equal(
			Number(asSent.headers.get("x-request-count")),
			storedFrom,
			"the identity request is served the same stored entry, not a fresh fetch",
		);
		t.equal(
			asSent.headers.get("content-encoding"),
			"gzip",
			"and receives the coding the origin sent, the entry having kept it",
		);
		const bytes = await asSent.bytes();
		t.deepEqual([bytes[0], bytes[1]], GZIP_MAGIC, "with the bytes as the origin sent them");
		t.equal(
			zlib.gunzipSync(bytes).toString("utf8"),
			PAYLOAD,
			"so one entry served both callers, decoded for one and as sent for the other",
		);
	});
});

test("encoding: a disk-store entry round-trips its coding", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		const dir = fs.mkdtempSync(path.join(os.tmpdir(), "faith-encoding-cache-"));
		const cached = agent({ cache: { store: "disk", path: dir } });

		const first = await fetch(origin.url("/cacheable/gzip"), {
			agent: cached,
			timeout: 10000,
		});
		t.equal(await first.text(), PAYLOAD, "written decoded to the caller");
		const firstCount = Number(first.headers.get("x-request-count"));

		const second = await fetch(origin.url("/cacheable/gzip"), {
			agent: cached,
			timeout: 10000,
		});
		t.equal(
			Number(second.headers.get("x-request-count")),
			firstCount,
			"the second is served off disk",
		);
		t.equal(await second.text(), PAYLOAD, "and decodes to the same representation");

		fs.rmSync(dir, { recursive: true, force: true });
	});
});
