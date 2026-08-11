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
  t.plan(2);

  try {
    // HEAD request has no body
    const response = await fetch(url("/get"), {
      method: "HEAD",
    });

    t.equal(response.body, null, "body should be null for empty response");
    t.equal(
      response.body,
      null,
      "body should still be null on a second access",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("body property returns the same stream on every access", async (t) => {
  t.plan(3);

  try {
    const response = await fetch(url("/get"));

    const first = response.body;
    t.ok(first, "body should return a stream");
    t.equal(
      response.body,
      first,
      "second access should return the same stream object",
    );
    t.equal(
      response.body,
      first,
      "further accesses should return the same stream object",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("body taken after a partial read continues from that point", async (t) => {
  t.plan(4);

  const total = 65536;

  try {
    const response = await fetch(
      url(`/stream-bytes/${total}?chunk_size=1024`),
    );

    // Read one chunk, then let go of the reader without finishing the body.
    const reader1 = response.body.getReader();
    const first = await reader1.read();
    t.ok(!first.done, "first read should return a chunk");
    t.ok(first.value.length > 0, "first chunk should carry bytes");
    t.ok(first.value.length < total, "first chunk should be part of the body");
    reader1.releaseLock();

    // A handle taken now picks up where the first left off, rather than
    // replaying the body from the start.
    let rest = 0;
    const reader2 = response.body.getReader();
    while (true) {
      const { done, value } = await reader2.read();
      if (done) break;
      rest += value.length;
    }
    reader2.releaseLock();

    t.equal(
      first.value.length + rest,
      total,
      "the two handles together should cover the body exactly once",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("a drained body stream stays drained on the next access", async (t) => {
  t.plan(2);

  try {
    const response = await fetch(url("/get"));

    const reader1 = response.body.getReader();
    let total = 0;
    while (true) {
      const { done, value } = await reader1.read();
      if (done) break;
      total += value.length;
    }
    reader1.releaseLock();
    t.ok(total > 0, "body should have carried bytes");

    const reader2 = response.body.getReader();
    const again = await reader2.read();
    t.ok(again.done, "a handle taken after the body ran out should be done");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
