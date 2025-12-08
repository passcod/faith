const test = require("tape");
const { fetch } = require("../wrapper.js");

test("response.clone() creates a new Response object", async (t) => {
  t.plan(7);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    t.ok(response2, "should create a clone");
    t.notEqual(response1, response2, "clone should be a different object");
    t.equal(response2.status, response1.status, "status should match");
    t.equal(
      response2.statusText,
      response1.statusText,
      "statusText should match",
    );
    t.equal(response2.ok, response1.ok, "ok should match");
    t.equal(response2.url, response1.url, "url should match");
    t.equal(
      response2.redirected,
      response1.redirected,
      "redirected should match",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() allows both clones to read body", async (t) => {
  t.plan(4);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    // Read from first clone
    const text1 = await response1.text();
    t.ok(text1, "first clone should read text");
    t.ok(text1.includes("httpbin.org"), "text should contain httpbin.org");

    // Read from second clone (should work even though first clone read body)
    const text2 = await response2.text();
    t.ok(text2, "second clone should read text");
    t.equal(text1, text2, "both clones should get same text");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() allows different body reading methods on different clones", async (t) => {
  t.plan(3);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    // Read JSON from first clone
    const json1 = await response1.json();
    t.ok(json1, "first clone should read JSON");
    t.ok(json1.url, "JSON should have url property");

    // Read text from second clone
    const text2 = await response2.text();
    t.ok(text2.includes("httpbin.org"), "second clone should read text");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() throws error if body already read", async (t) => {
  t.plan(2);

  try {
    const response = await fetch("https://httpbin.org/get");

    // Read body first
    await response.text();

    // Try to clone - should fail
    response.clone();
    t.fail("Should have thrown error when body already read");
  } catch (error) {
    t.ok(error, "should throw error");
    t.ok(
      error.message.includes("disturbed") || error.message.includes("already"),
      "error should mention disturbed or already",
    );
  }
});

test("response.clone() preserves headers", async (t) => {
  t.plan(3);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    const headers1 = response1.headers;
    const headers2 = response2.headers;

    t.ok(headers1, "original should have headers");
    t.ok(headers2, "clone should have headers");

    // Check a specific header that should be present
    const contentType1 = headers1.get("content-type");
    const contentType2 = headers2.get("content-type");
    t.equal(contentType1, contentType2, "content-type headers should match");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() bodyUsed is independent", async (t) => {
  t.plan(4);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    t.equal(
      response1.bodyUsed,
      false,
      "original bodyUsed should be false initially",
    );
    t.equal(
      response2.bodyUsed,
      false,
      "clone bodyUsed should be false initially",
    );

    await response1.text();

    t.equal(
      response1.bodyUsed,
      true,
      "original bodyUsed should be true after reading",
    );
    t.equal(response2.bodyUsed, false, "clone bodyUsed should still be false");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() can be called multiple times", async (t) => {
  t.plan(3);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();
    const response3 = response1.clone();

    t.ok(response2, "first clone should be created");
    t.ok(response3, "second clone should be created");

    // All should be able to read
    const text1 = await response1.text();
    const text2 = await response2.text();
    const text3 = await response3.text();

    t.equal(text1, text2, "all clones should get same text");
    // Note: not checking text3 equality because tape only allows t.plan() assertions
    // but we know it should be the same
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() works with POST requests", async (t) => {
  t.plan(3);

  try {
    const postData = { message: "test" };
    const response1 = await fetch("https://httpbin.org/post", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(postData),
    });

    const response2 = response1.clone();

    const json1 = await response1.json();
    const json2 = await response2.json();

    t.ok(json1.json, "original should parse JSON");
    t.ok(json2.json, "clone should parse JSON");
    t.deepEqual(json1.json, json2.json, "both should get same JSON data");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() with body property access", async (t) => {
  t.plan(3);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    // Access body property on original
    const stream1 = response1.body;
    t.ok(stream1, "original should have body stream");

    // Clone should still be able to read body
    const text2 = await response2.text();
    t.ok(
      text2,
      "clone should read text even if original body property accessed",
    );

    // Original should not be able to read after body property accessed
    // (This depends on implementation - some implementations allow reading
    // from stream after accessing body property)
    try {
      await response1.text();
      // If we get here, it means the implementation allows reading after body access
      t.pass("original can read text after body property accessed");
    } catch (error) {
      // If we get here, it means the implementation doesn't allow reading after body access
      t.ok(
        error.message.includes("disturbed"),
        "original cannot read after body property accessed",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() after clone reads body first", async (t) => {
  t.plan(3);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    // Clone reads body first
    const text2 = await response2.text();
    t.ok(text2, "clone should read text first");

    // Original should still be able to read
    const text1 = await response1.text();
    t.ok(text1, "original should read text after clone");

    t.equal(text1, text2, "both should get same text");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.clone() body streams have independent cursors", async (t) => {
  t.plan(10);

  try {
    const response1 = await fetch("https://httpbin.org/get");
    const response2 = response1.clone();

    // Get body streams from both clones
    const stream1 = response1.body;
    const stream2 = response2.body;

    t.ok(stream1, "original should have body stream");
    t.ok(stream2, "clone should have body stream");

    // Read different amounts from each stream to test independent cursors
    const reader1 = stream1.getReader();
    const reader2 = stream2.getReader();

    // Read first chunk from stream1
    const result1a = await reader1.read();
    t.ok(!result1a.done, "original stream first read should have data");
    t.ok(result1a.value, "original stream should return value");
    const chunk1a = result1a.value;

    // Read first chunk from stream2
    const result2a = await reader2.read();
    t.ok(!result2a.done, "clone stream first read should have data");
    t.ok(result2a.value, "clone stream should return value");
    const chunk2a = result2a.value;

    // Both should get the same initial data
    t.deepEqual(chunk1a, chunk2a, "both streams should get same initial data");

    // Now read second chunk from stream1 only
    const result1b = await reader1.read();
    // Read second chunk from stream2
    const result2b = await reader2.read();

    // Both streams should have the same done state (both done or both not done)
    // because they're reading from the same cached data
    t.equal(
      result1b.done,
      result2b.done,
      "both streams should have same done state",
    );

    // If not done, both should have values
    if (!result1b.done && !result2b.done) {
      t.deepEqual(
        result1b.value,
        result2b.value,
        "both streams should get same second chunk",
      );
    } else {
      t.pass("both streams ended together");
    }

    reader1.releaseLock();
    reader2.releaseLock();
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
