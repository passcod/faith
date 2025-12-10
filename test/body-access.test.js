const test = require("tape");
const { fetch, ERROR_CODES } = require("../wrapper.js");
const native = require("../index.js");
const { url, hostname } = require("./helpers.js");

test("body property access behavior", async (t) => {
  t.plan(8);

  try {
    // Test 1: Accessing body property should return a stream
    const response1 = await fetch(url("/get"));
    const bodyStream = response1.body;
    t.ok(bodyStream, "body property should return a stream");
    t.equal(
      typeof bodyStream.getReader,
      "function",
      "stream should have getReader method",
    );

    // Test 2: Accessing body should mark response as disturbed
    t.equal(
      response1.bodyUsed,
      true,
      "bodyUsed should be true after accessing body property",
    );

    // Test 3: Should not be able to clone after accessing body
    try {
      response1.clone();
      t.fail("Should have thrown error when cloning after body access");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.ResponseAlreadyDisturbed,
        "should set canonical error code 'ResponseAlreadyDisturbed'",
      );
    }

    // Test 4: Should not be able to read body again after accessing body property
    try {
      await response1.text();
      t.fail("Should have thrown error when reading after body access");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.ResponseAlreadyDisturbed,
        "should set canonical error code 'ResponseAlreadyDisturbed'",
      );
    }

    // Test 5: Clone created before body access should still work
    const response2 = await fetch(url("/get"));
    const response3 = response2.clone();

    // Access body on original
    const stream2 = response2.body;
    t.ok(stream2, "original should have body stream");

    // Clone should still be able to read
    const text3 = await response3.text();
    t.ok(text3, "clone should read text even if original body accessed");
    t.ok(text3.includes(hostname()), "text should contain expected content");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("body property returns null for empty responses", async (t) => {
  t.plan(1);

  try {
    // HEAD request has no body
    const response = await fetch(url("/get"), {
      method: "HEAD",
    });

    t.equal(response.body, null, "body should be null for empty response");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
