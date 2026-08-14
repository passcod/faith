/**
 * Request body compression: what the `compress` option puts on the wire.
 *
 * Compression is opt-in per request, so nothing here is about negotiation: the tests read the
 * bytes and headers the origin actually received (spec: ENC). They use our own origin rather
 * than go-httpbin, which decompresses request bodies on its own terms and reports what it
 * decoded rather than what arrived. See `test/fixtures/encoding-origin.js`.
 */

const test = require("tape");
const zlib = require("node:zlib");

const { fetch, Agent, ERROR_CODES } = require("../wrapper.js");
const { createEncodingOrigin, PAYLOAD } = require("./fixtures/encoding-origin.js");
const { streamingAgent } = require("./helpers.js");

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

/** Wire token to the decoder that unwinds it, so a test can read what was sent. */
const DECODERS = {
	gzip: zlib.gunzipSync,
	deflate: zlib.inflateSync,
	br: zlib.brotliDecompressSync,
	zstd: zlib.zstdDecompressSync,
};

/** Undo `codings` in reverse, the last applied being the first to come off. */
function decode(bytes, codings) {
	return codings.reduceRight((acc, coding) => DECODERS[coding](acc), bytes);
}

/** The `/sink` report, with the body it received as a Buffer. */
async function sink(res) {
	const seen = await res.json();
	return { ...seen, body: Buffer.from(seen.bodyBase64, "base64") };
}

for (const coding of ["gzip", "deflate", "br", "zstd"]) {
	test(`request compression: ${coding} names its coding and sends the compressed bytes`, async (t) => {
		await withOrigin(t, async ({ origin }) => {
			const res = await fetch(origin.url("/sink"), {
				method: "POST",
				body: PAYLOAD,
				compress: coding,
				timeout: 10000,
			});
			const seen = await sink(res);

			t.equal(
				seen.headers["content-encoding"],
				coding,
				"Content-Encoding names the coding Faith applied",
			);
			t.ok(
				seen.bodyLength < Buffer.byteLength(PAYLOAD),
				"and fewer bytes went out than were handed in",
			);
			t.equal(
				seen.headers["content-length"],
				String(seen.bodyLength),
				"Content-Length counts the compressed bytes",
			);
			t.equal(
				decode(seen.body, [coding]).toString("utf8"),
				PAYLOAD,
				"which decode to the body the caller supplied",
			);
		});
	});
}

test("request compression: the option is off unless asked for", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/sink"), {
			method: "POST",
			body: PAYLOAD,
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(
			seen.headers["content-encoding"],
			undefined,
			"no Content-Encoding without the option",
		);
		t.equal(seen.body.toString("utf8"), PAYLOAD, "and the body goes out as given");
	});
});

test("request compression: Faith's coding is layered on what the caller declares", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// The caller hands over bytes they gzipped themselves and says so; Faith puts zstd
		// over the top, so the header names both in the order they were applied.
		const supplied = zlib.gzipSync(Buffer.from(PAYLOAD, "utf8"));
		const res = await fetch(origin.url("/sink"), {
			method: "POST",
			body: supplied,
			headers: { "Content-Encoding": "gzip" },
			compress: "zstd",
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(
			seen.headers["content-encoding"],
			"gzip, zstd",
			"the caller's coding, then Faith's",
		);
		t.equal(
			decode(seen.body, ["gzip", "zstd"]).toString("utf8"),
			PAYLOAD,
			"and unwinding both gives back the original",
		);
		t.deepEqual(
			zlib.zstdDecompressSync(seen.body),
			supplied,
			"Faith compressed the bytes it was handed, not the ones underneath them",
		);
	});
});

test("request compression: the declared coding arrives on one header line", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/sink"), {
			method: "POST",
			body: zlib.gzipSync(Buffer.from(PAYLOAD, "utf8")),
			headers: { "Content-Encoding": "gzip" },
			compress: "br",
			timeout: 10000,
		});
		const seen = await sink(res);

		// Node joins repeated Content-Encoding lines with ", ", so a second line would read
		// as the caller's coding twice over.
		t.equal(seen.headers["content-encoding"], "gzip, br", "one list, named once");
	});
});

test("request compression: an agent's default Content-Encoding is what Faith layers on", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		const res = await fetch(origin.url("/sink"), {
			method: "POST",
			body: zlib.gzipSync(Buffer.from(PAYLOAD, "utf8")),
			compress: "zstd",
			agent: agent({ headers: [{ name: "Content-Encoding", value: "gzip" }] }),
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(
			seen.headers["content-encoding"],
			"gzip, zstd",
			"the agent's default is built on, not displaced",
		);
	});
});

test("request compression: a request header beats the agent's default", async (t) => {
	await withOrigin(t, async ({ origin, agent }) => {
		const res = await fetch(origin.url("/sink"), {
			method: "POST",
			body: zlib.brotliCompressSync(Buffer.from(PAYLOAD, "utf8")),
			headers: { "Content-Encoding": "br" },
			compress: "zstd",
			agent: agent({ headers: [{ name: "Content-Encoding", value: "gzip" }] }),
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(seen.headers["content-encoding"], "br, zstd", "per-name override, as ever");
	});
});

test("request compression: a request with no body sends no Content-Encoding", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/sink"), {
			compress: "gzip",
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(
			seen.headers["content-encoding"],
			undefined,
			"nothing was compressed, so nothing describes it",
		);
		t.equal(seen.bodyLength, 0, "and no bytes went out");
	});
});

test("request compression: a null body is no body", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/sink"), {
			method: "POST",
			body: null,
			compress: "zstd",
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(seen.headers["content-encoding"], undefined, "the option did nothing");
	});
});

test("request compression: a value naming no coding throws", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		for (const value of ["lzma", "brotli", "x-gzip", "GZIP", "identity", ""]) {
			try {
				await fetch(origin.url("/sink"), {
					method: "POST",
					body: PAYLOAD,
					compress: value,
					timeout: 10000,
				});
				t.fail(`compress: ${JSON.stringify(value)} should have thrown`);
			} catch (err) {
				t.equal(
					err.code,
					ERROR_CODES.InvalidCompression,
					`compress: ${JSON.stringify(value)} is InvalidCompression`,
				);
				t.ok(err instanceof TypeError, "and is a TypeError, as API misuse is");
			}
		}
	});
});

test("request compression: a bad value throws even with no body to compress", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		try {
			await fetch(origin.url("/sink"), { compress: "lzma", timeout: 10000 });
			t.fail("should have thrown");
		} catch (err) {
			t.equal(
				err.code,
				ERROR_CODES.InvalidCompression,
				"misuse is misuse whether or not there was a body",
			);
		}
	});
});

test("request compression: a streaming body goes out chunked", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const chunks = ["first chunk, ", "second chunk, ", "third chunk"];
		const stream = new ReadableStream({
			start(controller) {
				for (const chunk of chunks) controller.enqueue(Buffer.from(chunk, "utf8"));
				controller.close();
			},
		});

		const res = await fetch(origin.url("/sink"), {
			method: "POST",
			body: stream,
			duplex: "half",
			compress: "gzip",
			agent: streamingAgent(),
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(seen.headers["content-encoding"], "gzip", "the coding is named");
		t.equal(
			seen.headers["content-length"],
			undefined,
			"and no length is declared, there being none to know up front",
		);
		t.equal(seen.headers["transfer-encoding"], "chunked", "so the body is chunked");
		t.equal(
			decode(seen.body, ["gzip"]).toString("utf8"),
			chunks.join(""),
			"and the chunks decode to what was written",
		);
	});
});

test("request compression: a streaming body still needs HTTP/2 or HTTP/3", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		// Compressing changes nothing about the rule: the body still has no length to
		// declare, so the default agent refuses it over HTTP/1.1 (spec:REQ).
		const stream = new ReadableStream({
			start(controller) {
				controller.enqueue(Buffer.from("chunk", "utf8"));
				controller.close();
			},
		});

		try {
			await fetch(origin.url("/sink"), {
				method: "POST",
				body: stream,
				duplex: "half",
				compress: "gzip",
				timeout: 10000,
			});
			t.fail("should have been refused");
		} catch (err) {
			t.equal(err.code, ERROR_CODES.Network, "refused as a network error");
			t.equal(origin.count(), 0, "and the origin saw no request at all");
		}
	});
});

test("request compression: a server refusing the coding answers for itself", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/refuses-coding"), {
			method: "POST",
			body: PAYLOAD,
			compress: "zstd",
			timeout: 10000,
		});

		t.equal(res.status, 415, "the 415 reaches the caller");
		t.equal(
			res.headers.get("accept-encoding"),
			"gzip",
			"with the Accept-Encoding naming what the server would have taken",
		);
		t.deepEqual(
			await res.json(),
			{ refused: "zstd" },
			"and the body it sent, so retrying is the caller's to do",
		);
		t.equal(origin.count(), 1, "Faith sent nothing again of its own accord");
	});
});

test("request compression: a 307 replays the bytes already compressed", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/redirect-to-sink/307"), {
			method: "POST",
			body: PAYLOAD,
			compress: "gzip",
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(seen.method, "POST", "the method is preserved");
		t.equal(seen.headers["content-encoding"], "gzip", "under the coding as sent");
		t.equal(
			decode(seen.body, ["gzip"]).toString("utf8"),
			PAYLOAD,
			"and one unwind gives the body back, so it was not compressed twice",
		);
	});
});

test("request compression: a 303 drops Content-Encoding with the body", async (t) => {
	await withOrigin(t, async ({ origin }) => {
		const res = await fetch(origin.url("/redirect-to-sink/303"), {
			method: "POST",
			body: PAYLOAD,
			compress: "gzip",
			timeout: 10000,
		});
		const seen = await sink(res);

		t.equal(seen.method, "GET", "the request became a GET");
		t.equal(seen.bodyLength, 0, "the body is gone");
		t.equal(
			seen.headers["content-encoding"],
			undefined,
			"and nothing is left claiming to describe it",
		);
	});
});
