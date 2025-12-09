const test = require("tape");
const { fetch, ERROR_CODES } = require("../wrapper.js");
const native = require("../index.js");
const { url, hostname } = require("./helpers.js");

test("response.arrayBuffer() method returns ArrayBuffer", async (t) => {
  t.plan(5);

  try {
    const response = await fetch(url("/get"));

    const arrayBuffer = await response.arrayBuffer();

    t.ok(arrayBuffer, "should get ArrayBuffer");
    t.equal(
      arrayBuffer.constructor.name,
      "ArrayBuffer",
      "should return ArrayBuffer instance",
    );
    t.ok(arrayBuffer.byteLength > 0, "should have non-zero byteLength");

    // Convert to text to verify content
    const text = new TextDecoder().decode(arrayBuffer);
    t.ok(text.includes(hostname()), "arrayBuffer content should contain hostname");
    t.ok(text.includes('"url"'), "arrayBuffer content should be valid JSON");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.arrayBuffer() marks body as used", async (t) => {
  t.plan(3);

  try {
    const response = await fetch(url("/get"));

    t.equal(response.bodyUsed, false, "body should not be used initially");

    await response.arrayBuffer();

    t.equal(
      response.bodyUsed,
      true,
      "body should be marked as used after arrayBuffer()",
    );

    // Try to use arrayBuffer() again - should fail
    try {
      await response.arrayBuffer();
      t.fail("Should have thrown error when body already used");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.ResponseAlreadyDisturbed,
        "should set canonical error code 'ResponseAlreadyDisturbed'",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.arrayBuffer() and other methods are mutually exclusive", async (t) => {
  t.plan(3);

  try {
    const response = await fetch(url("/get"));

    // Use arrayBuffer() first
    await response.arrayBuffer();

    // Try to use text() - should fail
    try {
      await response.text();
      t.fail("Should have thrown error when body already used by arrayBuffer()");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.ResponseAlreadyDisturbed,
        "should set canonical error code 'ResponseAlreadyDisturbed'",
      );
    }

    // Try json() - should also fail
    try {
      await response.json();
      t.fail("Should have thrown error when body already used by arrayBuffer()");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.ResponseAlreadyDisturbed,
        "should set canonical error code 'ResponseAlreadyDisturbed'",
      );
    }

    // Try bytes() - should also fail
    try {
      await response.bytes();
      t.fail("Should have thrown error when body already used by arrayBuffer()");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.ResponseAlreadyDisturbed,
        "should set canonical error code 'ResponseAlreadyDisturbed'",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.arrayBuffer() and body property are mutually exclusive", async (t) => {
  t.plan(2);

  try {
    const response = await fetch(url("/get"));

    // Access body property first
    const stream = response.body;
    t.ok(stream, "should get stream from body property");

    // Try to use arrayBuffer() - should fail
    try {
      await response.arrayBuffer();
      t.fail("Should have thrown error when body property was accessed");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.ResponseAlreadyDisturbed,
        "should set canonical error code 'ResponseAlreadyDisturbed'",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.arrayBuffer() works with binary data", async (t) => {
  t.plan(3);

  try {
    // Get bytes response (returns random bytes)
    const response = await fetch(url("/bytes/100")); // 100 random bytes

    const arrayBuffer = await response.arrayBuffer();

    t.ok(arrayBuffer, "should get ArrayBuffer");
    t.equal(
      arrayBuffer.byteLength,
      100,
      "should have correct byteLength (100 bytes)",
    );

    // Verify it's actually an ArrayBuffer
    const uint8View = new Uint8Array(arrayBuffer);
    t.equal(uint8View.length, 100, "should be able to create Uint8Array view");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.arrayBuffer() with empty response", async (t) => {
  t.plan(2);

  try {
    // HEAD request has no body
    const response = await fetch(url("/get"), {
      method: "HEAD",
    });

    const arrayBuffer = await response.arrayBuffer();

    t.ok(arrayBuffer, "should get ArrayBuffer even for empty response");
    t.equal(
      arrayBuffer.byteLength,
      0,
      "should have zero byteLength for HEAD request",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.arrayBuffer() returns same data as bytes()", async (t) => {
  t.plan(3);

  try {
    // Get two responses to same endpoint
    const response1 = await fetch(url("/get"));
    const response2 = await fetch(url("/get"));

    const arrayBuffer = await response1.arrayBuffer();
    const bytes = await response2.bytes();

    t.ok(arrayBuffer, "should get ArrayBuffer");
    t.ok(bytes, "should get bytes Buffer");

    // Convert ArrayBuffer to Uint8Array for comparison
    const arrayBufferBytes = new Uint8Array(arrayBuffer);
    const bytesArray = new Uint8Array(bytes);

    // Both should have same length
    t.equal(
      arrayBufferBytes.length,
      bytesArray.length,
      "arrayBuffer and bytes should have same length",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("response.arrayBuffer() can be used with TypedArrays", async (t) => {
  t.plan(4);

  try {
    const response = await fetch(url("/bytes/100"));

    const arrayBuffer = await response.arrayBuffer();

    // Test different TypedArray views
    const uint8View = new Uint8Array(arrayBuffer);
    t.equal(uint8View.length, 100, "Uint8Array view should have correct length");

    const uint16View = new Uint16Array(arrayBuffer);
    t.equal(uint16View.length, 50, "Uint16Array view should have correct length");

    const uint32View = new Uint32Array(arrayBuffer);
    t.equal(uint32View.length, 25, "Uint32Array view should have correct length");

    const dataView = new DataView(arrayBuffer);
    t.equal(dataView.byteLength, 100, "DataView should have correct byteLength");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
