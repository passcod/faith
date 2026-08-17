/**
 * Test helpers for Faith fetch tests
 */

// Base URL for tests - HTTPBIN_URL environment variable is required
if (!process.env.HTTPBIN_URL) {
	throw new Error(
		"HTTPBIN_URL environment variable is required. Start httpbin with: podman run --rm -d -p 8888:8080 ghcr.io/mccutchen/go-httpbin",
	);
}
const HTTPBIN_BASE_URL = process.env.HTTPBIN_URL;

function url(path) {
	return `${HTTPBIN_BASE_URL}${path}`;
}

function hostname() {
	return new URL(HTTPBIN_BASE_URL).host;
}

function port() {
	const parsed = new URL(HTTPBIN_BASE_URL);
	return parsed.port || (parsed.protocol === "https:" ? "443" : "80");
}

// An agent that will stream a request body over HTTP/1.1.
//
// The httpbin origin these tests run against is plain HTTP/1.1, where a streaming request body
// is refused by default (spec:REQ#streaming-a-request-body). Tests exercising the streaming
// machinery itself opt into the quirk so that rule isn't the thing they end up measuring; the
// rule has its own tests in h1-request-streaming.test.js.
function streamingAgent() {
	const { Agent } = require("../wrapper.js");
	return new Agent({ quirks: { h1RequestStreaming: true } });
}

// Skip tests if native fetch is not available
const hasNativeFetch = typeof globalThis.fetch === "function";

/**
 * Node's own fetch, the baseline a comparison measures Faith against.
 *
 * On Windows this intermittently fails on the run's first call with a bare
 * `TypeError: fetch failed`, while Faith's request to the same URL a line earlier
 * succeeds and every later call in the same process succeeds too. One assertion then
 * takes the whole matrix down. What a comparison is for is what a response looks
 * like, not how reliably Node connects, so a failed attempt is retried once.
 *
 * The reason undici buries in `cause` is printed on the way past: tape prints neither
 * it nor an errno, which is what left the cause unidentified when this started. Only
 * CI has ever produced it, so the note is the only way it will be seen. It goes out as
 * a TAP comment so it lands in the log without derailing the stream.
 */
async function nativeFetch(resource, options) {
	try {
		return await globalThis.fetch(resource, options);
	} catch (error) {
		const cause = error.cause;
		const reason = cause?.code ?? cause?.message ?? cause ?? error.message;
		console.log(`# native fetch failed (${reason}), retrying once`);
		return await globalThis.fetch(resource, options);
	}
}

// Helper to compare responses
async function compareResponses(t, path, options = {}) {
	const { fetch: faithFetch } = require("../wrapper.js");
	const faithResponse = await faithFetch(url(path), options);
	const nativeResponse = await nativeFetch(url(path), options);

	// Compare basic properties
	t.equal(
		faithResponse.status,
		nativeResponse.status,
		`Status should match for ${url}`,
	);
	t.equal(faithResponse.ok, nativeResponse.ok, `ok should match for ${url}`);
	t.equal(
		faithResponse.redirected,
		nativeResponse.redirected,
		`redirected should match for ${url}`,
	);

	// Compare URL (may differ slightly due to redirects)
	t.ok(
		faithResponse.url.includes(new URL(HTTPBIN_BASE_URL).host),
		`Faith URL should contain ${new URL(HTTPBIN_BASE_URL).host}: ${faithResponse.url}`,
	);
	t.ok(
		nativeResponse.url.includes(new URL(HTTPBIN_BASE_URL).host),
		`Native URL should contain ${new URL(HTTPBIN_BASE_URL).host}: ${nativeResponse.url}`,
	);

	// Compare headers - check that faith has all the headers native has (except some that may differ)
	const faithHeaders = faithResponse.headers;
	const nativeHeaders = Object.fromEntries(nativeResponse.headers.entries());

	// Headers that commonly differ between implementations
	const ignoreHeaders = [
		"accept-encoding",
		"accept-language",
		"sec-fetch-mode",
		"sec-fetch-site",
		"user-agent",
		"x-amzn-trace-id",
		"date", // Date will differ between requests
		"content-length", // Content length may differ due to different headers
		"server", // Server header may differ
	];

	// Check each native header
	for (const [name, value] of Object.entries(nativeHeaders)) {
		if (ignoreHeaders.includes(name.toLowerCase())) {
			continue;
		}

		const faithHasHeader = faithHeaders.has(name);
		t.ok(faithHasHeader, `Faith should have header ${name} for ${url}`);

		if (faithHasHeader) {
			const faithHeaderValue = faithHeaders.get(name);
			t.equal(
				faithHeaderValue,
				value,
				`Header ${name} should match for ${url}`,
			);
		}
	}

	// Compare response body (as JSON if possible)
	try {
		const faithText = await faithResponse.text();
		const nativeText = await nativeResponse.text();

		// Try to parse as JSON for comparison
		const faithJson = JSON.parse(faithText);
		const nativeJson = JSON.parse(nativeText);

		// Compare specific fields that should match
		const compareFields = ["args", "origin", "url", "headers"];

		for (const field of compareFields) {
			if (
				faithJson[field] !== undefined &&
				nativeJson[field] !== undefined
			) {
				if (field === "headers") {
					// For headers field, compare specific headers
					const faithHeaders = faithJson.headers;
					const nativeHeaders = nativeJson.headers;

					// Compare headers that should match
					// Note: go-httpbin returns headers as arrays
					const headerFields = ["Accept", "Host"];
					for (const headerField of headerFields) {
						if (
							faithHeaders[headerField] !== undefined &&
							nativeHeaders[headerField] !== undefined
						) {
							// Use deepEqual to handle both string and array formats
							t.deepEqual(
								faithHeaders[headerField],
								nativeHeaders[headerField],
								`JSON header ${headerField} should match for ${url}`,
							);
						}
					}
				} else {
					if (
						typeof faithJson[field] === "object" &&
						faithJson[field] !== null
					) {
						t.deepEqual(
							faithJson[field],
							nativeJson[field],
							`${field} should match for ${url}`,
						);
					} else {
						t.equal(
							faithJson[field],
							nativeJson[field],
							`${field} should match for ${url}`,
						);
					}
				}
			}
		}
	} catch (error) {
		// If we can't parse as JSON, just compare text
		// This happens for non-JSON responses
		const faithText = await faithResponse.text();
		const nativeText = await nativeResponse.text();
		t.equal(faithText, nativeText, `Response text should match for ${url}`);
	}
}

module.exports = {
	hasNativeFetch,
	compareResponses,
	streamingAgent,
	url,
	hostname,
	port,
};
