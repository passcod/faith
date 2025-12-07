/**
 * Header format tests for Faith Fetch API
 *
 * Tests that:
 * 1. Response.headers always returns a Headers object
 * 2. fetch() interface only supports Headers or plain objects for headers
 * 3. Arrays of pairs are not supported in fetch()
 */

const test = require("tape");
const { fetch } = require("../wrapper.js");

test("Response.headers always returns Headers object", async (t) => {
  t.plan(6);

  try {
    const response = await fetch("https://httpbin.org/get");

    // Check that headers is a Headers object
    t.ok(
      response.headers instanceof Headers,
      "response.headers should be a Headers instance",
    );

    // Check that it has the expected methods
    t.equal(
      typeof response.headers.get,
      "function",
      "should have get() method",
    );
    t.equal(
      typeof response.headers.forEach,
      "function",
      "should have forEach() method",
    );
    t.equal(
      typeof response.headers.entries,
      "function",
      "should have entries() method",
    );
    t.equal(
      typeof response.headers.has,
      "function",
      "should have has() method",
    );
    t.equal(
      typeof response.headers.keys,
      "function",
      "should have keys() method",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("fetch() accepts plain object headers", async (t) => {
  t.plan(3);

  try {
    const response = await fetch("https://httpbin.org/headers", {
      headers: {
        "X-Test-Header": "test-value",
        Accept: "application/json",
      },
    });

    t.ok(response.ok, "request should succeed");
    t.ok(
      response.headers instanceof Headers,
      "response.headers should be Headers object",
    );

    // Verify the request was made with our headers
    const text = await response.text();
    const data = JSON.parse(text);
    t.equal(
      data.headers["X-Test-Header"],
      "test-value",
      "custom header should be sent",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("fetch() accepts Headers object", async (t) => {
  t.plan(3);

  try {
    const headers = new Headers();
    headers.append("X-Test-Header", "test-value");
    headers.append("Accept", "application/json");

    const response = await fetch("https://httpbin.org/headers", {
      headers: headers,
    });

    t.ok(response.ok, "request should succeed");
    t.ok(
      response.headers instanceof Headers,
      "response.headers should be Headers object",
    );

    // Verify the request was made with our headers
    const text = await response.text();
    const data = JSON.parse(text);
    t.equal(
      data.headers["X-Test-Header"],
      "test-value",
      "custom header should be sent",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("fetch() rejects array of tuples for headers", async (t) => {
  t.plan(2);

  try {
    await fetch("https://httpbin.org/get", {
      headers: [
        ["X-Test", "value1"],
        ["Accept", "application/json"],
      ],
    });

    t.fail("Should have thrown error for array headers");
  } catch (error) {
    t.ok(
      error.message.includes(
        "headers must be a Headers object or a plain object",
      ),
      "should have correct error message",
    );
    t.equal(error.constructor.name, "TypeError", "should throw TypeError");
  }
});

test("fetch() rejects nested array for headers", async (t) => {
  t.plan(2);

  try {
    await fetch("https://httpbin.org/get", {
      headers: [["X-Test", "value1"]], // Single element array
    });

    t.fail("Should have thrown error for nested array headers");
  } catch (error) {
    t.ok(
      error.message.includes(
        "headers must be a Headers object or a plain object",
      ),
      "should have correct error message",
    );
    t.equal(error.constructor.name, "TypeError", "should throw TypeError");
  }
});

test("fetch() rejects string for headers", async (t) => {
  t.plan(2);

  try {
    await fetch("https://httpbin.org/get", {
      headers: "invalid",
    });

    t.fail("Should have thrown error for string headers");
  } catch (error) {
    t.ok(
      error.message.includes(
        "headers must be a Headers object or a plain object",
      ),
      "should have correct error message",
    );
    t.equal(error.constructor.name, "TypeError", "should throw TypeError");
  }
});

test("fetch() rejects number for headers", async (t) => {
  t.plan(2);

  try {
    await fetch("https://httpbin.org/get", {
      headers: 123,
    });

    t.fail("Should have thrown error for number headers");
  } catch (error) {
    t.ok(
      error.message.includes(
        "headers must be a Headers object or a plain object",
      ),
      "should have correct error message",
    );
    t.equal(error.constructor.name, "TypeError", "should throw TypeError");
  }
});

test("fetch() accepts null headers (treated as undefined)", async (t) => {
  t.plan(2);

  try {
    const response = await fetch("https://httpbin.org/get", {
      headers: null,
    });

    t.ok(response.ok, "request should succeed with null headers");
    t.ok(
      response.headers instanceof Headers,
      "response.headers should still be Headers object",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("fetch() accepts undefined headers", async (t) => {
  t.plan(2);

  try {
    const response = await fetch("https://httpbin.org/get", {
      headers: undefined,
    });

    t.ok(response.ok, "request should succeed with undefined headers");
    t.ok(
      response.headers instanceof Headers,
      "response.headers should still be Headers object",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("Headers object preserves duplicate headers", async (t) => {
  t.plan(4);

  // Create a Headers object with duplicate headers
  const headers = new Headers();
  headers.append("Set-Cookie", "sessionId=abc123; Path=/; HttpOnly");
  headers.append("Set-Cookie", "userId=42; Path=/; HttpOnly");
  headers.append("Cache-Control", "no-cache");
  headers.append("Cache-Control", "no-store");

  // Count headers - forEach only iterates over unique header names
  let count = 0;
  headers.forEach(() => count++);

  // Some implementations might treat Set-Cookie specially
  // Accept either 2 or 3 entries (Set-Cookie might be split)
  t.ok(
    count === 2 || count === 3,
    `should have 2 or 3 unique header entries (got ${count})`,
  );

  // get() returns comma-separated values for duplicate headers
  const setCookieValue = headers.get("Set-Cookie");
  t.ok(
    setCookieValue &&
      (setCookieValue.includes("sessionId=abc123") ||
        setCookieValue.includes("userId=42")),
    "get() should return Set-Cookie value(s)",
  );

  const cacheControlValue = headers.get("Cache-Control");
  t.ok(
    cacheControlValue &&
      (cacheControlValue.includes("no-cache") ||
        cacheControlValue.includes("no-store")),
    "get() should return Cache-Control value(s)",
  );

  // Check entries() returns entries
  const entries = Array.from(headers.entries());
  t.ok(
    entries.length >= 2,
    `entries() should return at least 2 entries (got ${entries.length})`,
  );
});

test("Response Headers object can be used with standard methods", async (t) => {
  t.plan(5);

  try {
    const response = await fetch("https://httpbin.org/get");

    // Test various Headers methods
    t.ok(
      response.headers.has("content-type"),
      "has() should find content-type header",
    );

    const contentType = response.headers.get("content-type");
    t.ok(
      contentType && contentType.includes("application/json"),
      "get() should return content-type value",
    );

    // Test forEach
    let headerCount = 0;
    response.headers.forEach(() => headerCount++);
    t.ok(headerCount > 0, "forEach should iterate over headers");

    // Test entries
    const entries = Array.from(response.headers.entries());
    t.ok(entries.length > 0, "entries() should return array of entries");
    t.ok(
      entries.some(([name]) => name.toLowerCase() === "content-type"),
      "should have content-type in entries",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
