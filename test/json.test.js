const { url } = require("./helpers.js");
/**
 * Test for Response.json() method
 */

const test = require("tape");
const { fetch } = require("../wrapper.js");

test("response.json() method returns parsed JSON", async (t) => {
  t.plan(4);

  try {
    const response = await fetch(url("/get"));

    // Test json() method
    const data = await response.json();

    t.ok(data, "should get JSON data");
    t.equal(typeof data, "object", "should return object");
    t.ok(data.url, "JSON should have url property");
    t.ok(
      data.url.includes(new URL(url("/")).hostname + "/get"),
      "url should be correct",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.json() throws error for non-JSON response", async (t) => {
  t.plan(2);

  try {
    // Get a plain text response (not JSON)
    const response = await fetch(url("/html"));

    // This should fail because the response is HTML, not JSON
    await response.json();

    t.fail("Should have thrown error for non-JSON response");
  } catch (error) {
    t.ok(error, "should throw error for non-JSON response");
    t.ok(
      error.message.includes("JSON") ||
        error.message.includes("parse") ||
        error.message.includes("invalid") ||
        error.message.includes("expected"),
      "error should mention JSON parsing or invalid/expected",
    );
  }
});

test("response.json() works with POST request returning JSON", async (t) => {
  t.plan(3);

  try {
    const postData = { message: "Hello from faith", number: 42 };
    const response = await fetch(url("/post"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(postData),
    });

    const data = await response.json();

    t.ok(data, "should get JSON data");
    t.ok(data.json, "should have json field in response");
    t.deepEqual(data.json, postData, "should return posted data");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.json() marks body as used", async (t) => {
  t.plan(3);

  try {
    const response = await fetch(url("/get"));

    t.equal(response.bodyUsed, false, "body should not be used initially");

    await response.json();

    t.equal(
      response.bodyUsed,
      true,
      "body should be marked as used after json()",
    );

    // Try to use json() again - should fail
    try {
      await response.json();
      t.fail("Should have thrown error when body already used");
    } catch (error) {
      t.ok(
        error.message.includes("disturbed") ||
          error.message.includes("already"),
        "should throw error about disturbed body",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.json() and text() are mutually exclusive", async (t) => {
  t.plan(2);

  try {
    const response = await fetch(url("/get"));

    // Use json() first
    await response.json();

    // Try to use text() - should fail
    try {
      await response.text();
      t.fail("Should have thrown error when body already used by json()");
    } catch (error) {
      t.ok(
        error.message.includes("disturbed") ||
          error.message.includes("already"),
        "should throw error about disturbed body",
      );
    }

    // Try bytes() - should also fail
    try {
      await response.bytes();
      t.fail("Should have thrown error when body already used by json()");
    } catch (error) {
      t.ok(
        error.message.includes("disturbed") ||
          error.message.includes("already"),
        "should throw error about disturbed body",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.json() and body property are mutually exclusive", async (t) => {
  t.plan(2);

  try {
    const response = await fetch(url("/get"));

    // Access body property first
    const stream = response.body;
    t.ok(stream, "should get stream from body property");

    // Try to use json() - should fail
    try {
      await response.json();
      t.fail("Should have thrown error when body property was accessed");
    } catch (error) {
      t.ok(
        error.message.includes("disturbed") ||
          error.message.includes("already"),
        "should throw error about disturbed body",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.json() handles empty JSON object", async (t) => {
  t.plan(2);

  try {
    // We'll use a POST request with empty JSON body
    const response = await fetch(url("/post"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: "{}", // Empty JSON
    });

    const data = await response.json();

    t.ok(data, "should get JSON data");
    t.ok(data.json, "should have json field (even if empty)");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.json() handles nested JSON structures", async (t) => {
  t.plan(4);

  try {
    const nestedData = {
      level1: {
        level2: {
          level3: {
            message: "deeply nested",
            numbers: [1, 2, 3, 4, 5],
            flag: true,
          },
        },
        items: ["a", "b", "c"],
      },
      timestamp: Date.now(),
    };

    const response = await fetch(url("/post"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(nestedData),
    });

    const data = await response.json();

    t.ok(data, "should get JSON data");
    t.ok(data.json, "should have json field");
    t.deepEqual(
      data.json.level1.level2.level3.message,
      "deeply nested",
      "should parse nested strings",
    );
    t.deepEqual(
      data.json.level1.level2.level3.numbers,
      [1, 2, 3, 4, 5],
      "should parse nested arrays",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
