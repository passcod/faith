/**
 * Test helpers for Faith fetch tests
 */

// Base URL for tests - HTTPBIN_URL environment variable is required
if (!process.env.HTTPBIN_URL) {
	throw new Error(
		"HTTPBIN_URL environment variable is required. Start httpbin with: docker run --rm -d -p 8888:80 ghcr.io/mccutchen/go-httpbin",
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

// Skip tests if native fetch is not available
const hasNativeFetch = typeof globalThis.fetch === "function";

// Helper to compare responses
async function compareResponses(t, path, options = {}) {
	const { fetch: faithFetch } = require("../wrapper.js");
	const faithResponse = await faithFetch(url(path), options);
	const nativeResponse = await globalThis.fetch(url(path), options);

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
	url,
	hostname,
	port,
};
