const test = require("tape");
const { fetch } = require("../wrapper.js");

test("response.blob() method returns Blob", async (t) => {
  t.plan(5);

  try {
    const response = await fetch("https://httpbin.org/get");

    // Test blob() method
    const blob = await response.blob();

    t.ok(blob, "should get Blob");
    t.equal(blob.constructor.name, "Blob", "should return Blob instance");
    t.ok(blob.size > 0, "should have non-zero size");
    t.ok(
      blob.type === "" || blob.type === "application/json",
      "should have empty or application/json type",
    );

    // Read the blob as text to verify content
    const text = await blob.text();
    t.ok(
      text.includes("httpbin.org"),
      "blob content should contain httpbin.org",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.blob() with content-type header", async (t) => {
  t.plan(3);

  try {
    // Get a response that has content-type
    const response = await fetch("https://httpbin.org/json");

    const blob = await response.blob();

    t.ok(blob, "should get Blob");
    t.ok(blob.size > 0, "should have non-zero size");
    // httpbin.org/json returns application/json content-type
    t.equal(blob.type, "application/json", "should have correct content-type");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.blob() marks body as used", async (t) => {
  t.plan(3);

  try {
    const response = await fetch("https://httpbin.org/get");

    t.equal(response.bodyUsed, false, "body should not be used initially");

    await response.blob();

    t.equal(
      response.bodyUsed,
      true,
      "body should be marked as used after blob()",
    );

    // Try to use blob() again - should fail
    try {
      await response.blob();
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

test("response.blob() and other methods are mutually exclusive", async (t) => {
  t.plan(2);

  try {
    const response = await fetch("https://httpbin.org/get");

    // Use blob() first
    await response.blob();

    // Try to use text() - should fail
    try {
      await response.text();
      t.fail("Should have thrown error when body already used by blob()");
    } catch (error) {
      t.ok(
        error.message.includes("disturbed") ||
          error.message.includes("already"),
        "should throw error about disturbed body",
      );
    }

    // Try json() - should also fail
    try {
      await response.json();
      t.fail("Should have thrown error when body already used by blob()");
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

test("response.blob() and body property are mutually exclusive", async (t) => {
  t.plan(2);

  try {
    const response = await fetch("https://httpbin.org/get");

    // Access body property first
    const stream = response.body;
    t.ok(stream, "should get stream from body property");

    // Try to use blob() - should fail
    try {
      await response.blob();
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

test("response.blob() works with binary data", async (t) => {
  t.plan(4);

  try {
    // Get bytes response (httpbin.org/bytes returns random bytes)
    const response = await fetch("https://httpbin.org/bytes/100"); // 100 random bytes

    const blob = await response.blob();

    t.ok(blob, "should get Blob");
    t.equal(blob.size, 100, "should have correct size (100 bytes)");
    t.ok(
      blob.type === "application/octet-stream" || blob.type === "",
      "should have octet-stream or empty type for binary data",
    );

    // Verify we can read it as array buffer
    const arrayBuffer = await blob.arrayBuffer();
    t.equal(
      arrayBuffer.byteLength,
      100,
      "should be able to read 100 bytes from blob",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.blob() preserves content-type from response headers", async (t) => {
  t.plan(2);

  try {
    // Test with image (httpbin.org/image returns an image with correct content-type)
    const response = await fetch("https://httpbin.org/image/jpeg");

    const blob = await response.blob();

    t.ok(blob, "should get Blob");
    // httpbin.org/image/jpeg returns image/jpeg content-type
    t.ok(
      blob.type === "image/jpeg" || blob.type === "",
      "should preserve image/jpeg content-type if available",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.blob() with empty response", async (t) => {
  t.plan(3);

  try {
    // HEAD request has no body
    const response = await fetch("https://httpbin.org/get", {
      method: "HEAD",
    });

    const blob = await response.blob();

    t.ok(blob, "should get Blob even for empty response");
    t.equal(blob.size, 0, "should have zero size for HEAD request");
    t.ok(
      blob.type === "" || blob.type === "application/json",
      "should have empty or application/json type for empty response",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
