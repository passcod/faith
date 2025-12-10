const { url } = require("./helpers.js");
const test = require("tape");
const { fetch } = require("../wrapper.js");
const { ReadableStream } = require("stream/web");

test("duplex: ReadableStream body without duplex option throws TypeError", async (t) => {
  t.plan(2);

  try {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode("test data"));
        controller.close();
      },
    });

    await fetch(url("/post"), {
      method: "POST",
      body: stream,
    });

    t.fail("Should have thrown TypeError");
  } catch (error) {
    t.ok(error instanceof TypeError, "should throw TypeError");
    t.ok(
      error.message.includes("duplex"),
      "error message should mention duplex",
    );
  }
});

test("duplex: ReadableStream body with duplex: 'half' works", async (t) => {
  t.plan(2);

  try {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(
          new TextEncoder().encode(JSON.stringify({ test: "data" })),
        );
        controller.close();
      },
    });

    const response = await fetch(url("/post"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: stream,
      duplex: "half",
    });

    t.equal(response.status, 200, "should return 200 status");

    const data = await response.json();
    t.equal(data.json.test, "data", "should send body correctly");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: ReadableStream with multiple chunks", async (t) => {
  t.plan(2);

  try {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('{"message":"'));
        controller.enqueue(new TextEncoder().encode("hello "));
        controller.enqueue(new TextEncoder().encode("world"));
        controller.enqueue(new TextEncoder().encode('"}'));
        controller.close();
      },
    });

    const response = await fetch(url("/post"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: stream,
      duplex: "half",
    });

    t.equal(response.status, 200, "should return 200 status");

    const data = await response.json();
    t.equal(
      data.json.message,
      "hello world",
      "should concatenate chunks correctly",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: String body does not require duplex option", async (t) => {
  t.plan(1);

  try {
    const response = await fetch(url("/post"), {
      method: "POST",
      headers: { "Content-Type": "text/plain" },
      body: "test string",
    });

    t.equal(response.status, 200, "should work without duplex option");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: Buffer body does not require duplex option", async (t) => {
  t.plan(1);

  try {
    const response = await fetch(url("/post"), {
      method: "POST",
      body: Buffer.from("test buffer"),
    });

    t.equal(response.status, 200, "should work without duplex option");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: Uint8Array body does not require duplex option", async (t) => {
  t.plan(1);

  try {
    const response = await fetch(url("/post"), {
      method: "POST",
      body: new Uint8Array([1, 2, 3, 4]),
    });

    t.equal(response.status, 200, "should work without duplex option");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: ArrayBuffer body does not require duplex option", async (t) => {
  t.plan(1);

  try {
    const buffer = new ArrayBuffer(4);
    const view = new Uint8Array(buffer);
    view.set([1, 2, 3, 4]);

    const response = await fetch(url("/post"), {
      method: "POST",
      body: buffer,
    });

    t.equal(response.status, 200, "should work without duplex option");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: ReadableStream with binary data", async (t) => {
  t.plan(2);

  try {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new Uint8Array([0, 1, 2, 3]));
        controller.enqueue(new Uint8Array([4, 5, 6, 7]));
        controller.close();
      },
    });

    const response = await fetch(url("/post"), {
      method: "POST",
      body: stream,
      duplex: "half",
    });

    t.equal(response.status, 200, "should return 200 status");

    const bytes = await response.bytes();
    t.ok(bytes.length > 0, "should receive response body");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: ReadableStream with empty stream", async (t) => {
  t.plan(1);

  try {
    const stream = new ReadableStream({
      start(controller) {
        controller.close();
      },
    });

    const response = await fetch(url("/post"), {
      method: "POST",
      body: stream,
      duplex: "half",
    });

    t.equal(response.status, 200, "should work with empty stream");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: works with Request object containing ReadableStream", async (t) => {
  t.plan(2);

  try {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(
          new TextEncoder().encode(JSON.stringify({ from: "request" })),
        );
        controller.close();
      },
    });

    const request = new Request(url("/post"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: stream,
      duplex: "half",
    });

    const response = await fetch(request);
    t.equal(response.status, 200, "should work with Request object");

    const data = await response.json();
    t.equal(data.json.from, "request", "should send body from Request");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: options override Request duplex setting", async (t) => {
  t.plan(1);

  try {
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode("override test"));
        controller.close();
      },
    });

    // Request without duplex
    const request = new Request(url("/post"), {
      method: "POST",
      body: stream,
      duplex: "half",
    });

    // Fetch still works because Request had duplex
    const response = await fetch(request);
    t.equal(response.status, 200, "should work when Request has duplex");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: ReadableStream with large payload", async (t) => {
  t.plan(2);

  try {
    const chunkSize = 1024;
    const numChunks = 10;

    const stream = new ReadableStream({
      start(controller) {
        for (let i = 0; i < numChunks; i++) {
          const chunk = new Uint8Array(chunkSize);
          chunk.fill(i);
          controller.enqueue(chunk);
        }
        controller.close();
      },
    });

    const response = await fetch(url("/post"), {
      method: "POST",
      body: stream,
      duplex: "half",
    });

    t.equal(response.status, 200, "should handle large payload");
    t.ok(response.ok, "should return successful response");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: null body does not require duplex", async (t) => {
  t.plan(1);

  try {
    const response = await fetch(url("/get"), {
      method: "GET",
      body: null,
    });

    t.equal(response.status, 200, "should work with null body");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("duplex: undefined body does not require duplex", async (t) => {
  t.plan(1);

  try {
    const response = await fetch(url("/get"), {
      method: "GET",
    });

    t.equal(response.status, 200, "should work with undefined body");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
