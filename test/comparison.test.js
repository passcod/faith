const test = require("tape");
const { fetch: faithFetch } = require("../index.js");

// Skip tests if native fetch is not available
const hasNativeFetch = typeof globalThis.fetch === "function";

// Helper to compare responses
async function compareResponses(t, url, options = {}) {
  const faithResponse = await faithFetch(url, options);
  const nativeResponse = await globalThis.fetch(url, options);

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
    faithResponse.url.includes("httpbin.org"),
    `Faith URL should contain httpbin.org: ${faithResponse.url}`,
  );
  t.ok(
    nativeResponse.url.includes("httpbin.org"),
    `Native URL should contain httpbin.org: ${nativeResponse.url}`,
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

  for (const [key, value] of Object.entries(nativeHeaders)) {
    const lowerKey = key.toLowerCase();
    if (!ignoreHeaders.includes(lowerKey)) {
      t.ok(
        faithHeaders[lowerKey] !== undefined,
        `Faith should have header ${key}`,
      );
      if (faithHeaders[lowerKey] !== undefined) {
        t.equal(faithHeaders[lowerKey], value, `Header ${key} should match`);
      }
    }
  }

  // Compare response text - get faith body as text
  const faithText = await faithResponse.text();
  const nativeText = await nativeResponse.text();

  // For JSON responses, parse and compare structure (not exact text due to formatting differences)
  if (faithText.trim().startsWith("{") || faithText.trim().startsWith("[")) {
    try {
      const faithJson = JSON.parse(faithText);
      const nativeJson = JSON.parse(nativeText);

      // Compare common fields
      if (faithJson.args !== undefined && nativeJson.args !== undefined) {
        t.deepEqual(
          faithJson.args,
          nativeJson.args,
          `args should match for ${url}`,
        );
      }

      if (faithJson.origin !== undefined && nativeJson.origin !== undefined) {
        t.equal(
          faithJson.origin,
          nativeJson.origin,
          `origin should match for ${url}`,
        );
      }

      if (faithJson.url !== undefined && nativeJson.url !== undefined) {
        t.equal(faithJson.url, nativeJson.url, `url should match for ${url}`);
      }

      // Compare headers in JSON response if present
      if (faithJson.headers && nativeJson.headers) {
        for (const [key, value] of Object.entries(nativeJson.headers)) {
          const lowerKey = key.toLowerCase();
          if (
            !ignoreHeaders.includes(lowerKey) &&
            faithJson.headers[key] !== undefined
          ) {
            t.equal(
              faithJson.headers[key],
              value,
              `JSON header ${key} should match`,
            );
          }
        }
      }
    } catch (e) {
      // If JSON parsing fails, compare text directly
      t.equal(faithText, nativeText, `Response text should match for ${url}`);
    }
  } else {
    // For non-JSON responses, compare text directly
    t.equal(faithText, nativeText, `Response text should match for ${url}`);
  }
}

// Basic GET request comparison
test("Compare basic GET request", { skip: !hasNativeFetch }, async (t) => {
  // Don't set a fixed plan since compareResponses has variable assertions
  await compareResponses(t, "https://httpbin.org/get");
});

// GET request with query parameters
test("Compare GET with query params", { skip: !hasNativeFetch }, async (t) => {
  await compareResponses(t, "https://httpbin.org/get?foo=bar&baz=qux");
});

// Headers test
test(
  "Compare request with custom headers",
  { skip: !hasNativeFetch },
  async (t) => {
    await compareResponses(t, "https://httpbin.org/headers", {
      headers: {
        "X-Custom-Header": "test-value",
      },
    });
  },
);

// POST request with JSON body
test(
  "Compare POST request with JSON body",
  { skip: !hasNativeFetch },
  async (t) => {
    const testData = { message: "Hello from faith", number: 42 };

    await compareResponses(t, "https://httpbin.org/post", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: Array.from(Buffer.from(JSON.stringify(testData))),
    });
  },
);

// Test error handling
test("Compare error responses", { skip: !hasNativeFetch }, async (t) => {
  t.plan(2);

  // Test with invalid URL (should fail for both)
  let faithError = null;
  let nativeError = null;

  try {
    await faithFetch("https://invalid-domain-that-does-not-exist-12345.com/");
  } catch (error) {
    faithError = error;
  }

  try {
    await globalThis.fetch(
      "https://invalid-domain-that-does-not-exist-12345.com/",
    );
  } catch (error) {
    nativeError = error;
  }

  t.ok(faithError, "Faith should throw error for invalid URL");
  t.ok(nativeError, "Native fetch should throw error for invalid URL");
});

// Test different HTTP methods
test("Compare different HTTP methods", { skip: !hasNativeFetch }, async (t) => {
  const methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];

  for (const method of methods) {
    const url = `https://httpbin.org/${method.toLowerCase()}`;
    const options = { method };

    if (method === "POST" || method === "PUT" || method === "PATCH") {
      options.body = Array.from(Buffer.from("test body"));
    }

    await compareResponses(t, url, options);
  }
});

// Test response.text() method
test("Test response.text() method", async (t) => {
  t.plan(3);

  const response = await faithFetch("https://httpbin.org/get");
  const text = await response.text();

  t.ok(typeof text === "string", "body should be convertible to string");
  t.ok(text.length > 0, "body should contain data");
  t.ok(text.includes("httpbin.org"), "body should contain response data");
});

// Test response properties
test("Test response properties", async (t) => {
  t.plan(7);

  const response = await faithFetch("https://httpbin.org/get");

  t.equal(typeof response.status, "number", "status should be a number");
  t.equal(
    typeof response.statusText,
    "string",
    "statusText should be a string",
  );
  t.equal(typeof response.ok, "boolean", "ok should be a boolean");
  t.equal(typeof response.url, "string", "url should be a string");
  t.equal(
    typeof response.redirected,
    "boolean",
    "redirected should be a boolean",
  );
  t.equal(typeof response.timestamp, "number", "timestamp should be a number");
  t.equal(typeof response.body, "function", "body should be a function");
});

// Test with timeout
test("Compare request with timeout", { skip: !hasNativeFetch }, async (t) => {
  // Use a short timeout
  try {
    await compareResponses(t, "https://httpbin.org/delay/1", {
      timeout: 0.5, // 500ms timeout for a 1-second delay
    });
  } catch (error) {
    // Both should timeout
    t.pass("Request should timeout");
  }
});
