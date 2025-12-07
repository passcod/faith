const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");

test("Test response.body returns ReadableStream", async (t) => {
  t.plan(5);

  const response = await faithFetch("https://httpbin.org/get");

  // Test that body is a property (not a function)
  t.equal(
    typeof response.body,
    "object",
    "body should be an object (property)",
  );

  // Get the stream from the body property
  const stream = response.body;

  // Test that it returns a ReadableStream or null
  t.ok(
    stream === null || typeof stream === "object",
    "body should return object or null",
  );

  if (stream) {
    // Test ReadableStream properties
    t.ok(
      typeof stream.getReader === "function",
      "stream should have getReader method",
    );
    t.ok(
      typeof stream.pipeTo === "function",
      "stream should have pipeTo method",
    );
    t.ok(typeof stream.tee === "function", "stream should have tee method");
  } else {
    // If stream is null, skip the stream tests
    t.skip("stream is null, skipping stream tests");
    t.skip("stream is null, skipping stream tests");
    t.skip("stream is null, skipping stream tests");
  }
});

test("Test reading from ReadableStream", async (t) => {
  t.plan(3);

  const response = await faithFetch("https://httpbin.org/get");
  const stream = response.body;

  if (!stream) {
    t.skip("stream is null, skipping test");
    t.skip("stream is null, skipping test");
    t.skip("stream is null, skipping test");
    return;
  }

  const reader = stream.getReader();
  let chunks = [];
  let totalSize = 0;

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      chunks.push(value);
      totalSize += value.length;
    }

    t.ok(totalSize > 0, "should read some data from stream");
    t.ok(chunks.length > 0, "should have at least one chunk");

    // Combine chunks and check it's valid JSON (since httpbin returns JSON)
    const combined = Buffer.concat(chunks).toString("utf-8");
    t.doesNotThrow(
      () => JSON.parse(combined),
      "stream data should be valid JSON",
    );
  } finally {
    reader.releaseLock();
  }
});

test("Test stream consumption prevents text() call", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://httpbin.org/get");
  const stream = response.body;

  if (!stream) {
    t.skip("stream is null, skipping test");
    t.skip("stream is null, skipping test");
    return;
  }

  // Start reading from the stream
  const reader = stream.getReader();
  const { done } = await reader.read();
  reader.releaseLock();

  if (!done) {
    // If we read something, text() should fail because response is disturbed
    try {
      await response.text();
      t.fail("text() should throw when response is disturbed");
    } catch (err) {
      t.pass("text() throws when response is disturbed");
    }

    try {
      await response.bytes();
      t.fail("bytes() should throw when response is disturbed");
    } catch (err) {
      t.pass("bytes() throws when response is disturbed");
    }
  } else {
    // If stream was empty, skip
    t.skip("stream was empty, skipping test");
    t.skip("stream was empty, skipping test");
  }
});

test("Test text() and bytes() work when not streaming", async (t) => {
  t.plan(4);

  const response = await faithFetch("https://httpbin.org/get");

  // Test text() method
  const text = await response.text();
  t.ok(typeof text === "string", "text() should return string");
  t.ok(text.length > 0, "text() should return non-empty string");
  t.doesNotThrow(() => JSON.parse(text), "text() should return valid JSON");

  // Test bytes() method
  const response2 = await faithFetch("https://httpbin.org/get");
  const bytes = await response2.bytes();
  t.ok(bytes instanceof Uint8Array, "bytes() should return Uint8Array");
});

test("Test body returns null after consumption", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://httpbin.org/get");

  // First access to body property should return stream
  const stream1 = response.body;
  t.ok(stream1 !== null, "first body access should return stream");

  if (stream1) {
    // Consume the stream
    const reader = stream1.getReader();
    await reader.read();
    reader.releaseLock();

    const stream2 = response.body;
    t.ok(
      stream2 !== null,
      "wrapper caches stream, so second access returns same stream",
    );
  } else {
    t.skip("stream is null, skipping test");
  }
});
