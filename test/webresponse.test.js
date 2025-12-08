const { url, hostname } = require("./helpers.js");
/**
 * webResponse() method tests for Faith Fetch API
 *
 * Tests that:
 * 1. webResponse() returns a Web API Response object
 * 2. webResponse() throws error if body has been disturbed
 * 3. The returned Response has correct properties
 * 4. The returned Response can be used with standard Web API methods
 */

const test = require("tape");
const { fetch } = require("../wrapper.js");
const native = require("../index.js");

test("webResponse() returns Web API Response object", async (t) => {
  t.plan(8);

  try {
    const faithResponse = await fetch(url("/get"));

    // Get Web API Response
    const webResponse = faithResponse.webResponse();

    // Check it's a Web API Response
    t.ok(
      webResponse instanceof globalThis.Response,
      "should return Web API Response instance",
    );

    // Check properties match
    t.equal(webResponse.status, faithResponse.status, "status should match");
    t.equal(
      webResponse.statusText,
      faithResponse.statusText,
      "statusText should match",
    );
    t.equal(webResponse.ok, faithResponse.ok, "ok should match");

    // Check headers
    t.ok(
      webResponse.headers instanceof Headers,
      "headers should be Headers object",
    );

    // Check that we can use the Web API Response
    t.equal(typeof webResponse.text, "function", "should have text() method");
    t.equal(typeof webResponse.json, "function", "should have json() method");
    t.equal(
      typeof webResponse.arrayBuffer,
      "function",
      "should have arrayBuffer() method",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("webResponse() preserves headers correctly", async (t) => {
  t.plan(3);

  try {
    const faithResponse = await fetch(url("/get"));
    const webResponse = faithResponse.webResponse();

    // Get a specific header from both responses
    const faithContentType = faithResponse.headers.get("content-type");
    const webContentType = webResponse.headers.get("content-type");

    t.equal(
      webContentType,
      faithContentType,
      "content-type header should match",
    );

    // Check all headers are present
    const faithHeaders = Object.fromEntries(faithResponse.headers.entries());
    const webHeaders = Object.fromEntries(webResponse.headers.entries());

    // Check a few key headers
    t.ok(
      webHeaders["content-type"],
      "Web Response should have content-type header",
    );
    t.ok(
      webHeaders["content-length"],
      "Web Response should have content-length header",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("webResponse() body can be read", async (t) => {
  t.plan(3);

  try {
    const faithResponse = await fetch(url("/get"));
    const webResponse = faithResponse.webResponse();

    // Read body from Web API Response
    const text = await webResponse.text();

    t.ok(text, "should get response text");
    t.ok(text.length > 0, "text should not be empty");
    t.ok(text.includes(hostname()), "text should contain expected content");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("webResponse() throws error if body already consumed via text()", async (t) => {
  t.plan(2);

  try {
    const faithResponse = await fetch(url("/get"));

    // Consume body via text()
    await faithResponse.text();

    // Try to get webResponse() after body is consumed
    faithResponse.webResponse();

    t.fail("Should have thrown error");
  } catch (error) {
    t.equal(
      error.message,
      native.errResponseBodyNotAvailable(),
      "should throw 'Response body no longer available' error",
    );
    t.equal(
      error.code,
      native.errorCodes().response_body_not_available,
      "should set canonical error code 'response_body_not_available'",
    );
    t.equal(error.constructor.name, "Error", "should throw Error");
  }
});

test("webResponse() throws error if body already consumed via bytes()", async (t) => {
  t.plan(2);

  try {
    const faithResponse = await fetch(url("/get"));

    // Consume body via bytes()
    await faithResponse.bytes();

    // Try to get webResponse() after body is consumed
    faithResponse.webResponse();

    t.fail("Should have thrown error");
  } catch (error) {
    t.equal(
      error.message,
      native.errResponseBodyNotAvailable(),
      "should throw 'Response body no longer available' error",
    );
    t.equal(
      error.code,
      native.errorCodes().response_body_not_available,
      "should set canonical error code 'response_body_not_available'",
    );
    t.equal(error.constructor.name, "Error", "should throw Error");
  }
});

test("webResponse() throws error if body already consumed via arrayBuffer()", async (t) => {
  t.plan(2);

  try {
    const faithResponse = await fetch(url("/get"));

    // Consume body via arrayBuffer()
    await faithResponse.arrayBuffer();

    // Try to get webResponse() after body is consumed
    faithResponse.webResponse();

    t.fail("Should have thrown error");
  } catch (error) {
    t.equal(
      error.message,
      native.errResponseBodyNotAvailable(),
      "should throw 'Response body no longer available' error",
    );
    t.equal(
      error.code,
      native.errorCodes().response_body_not_available,
      "should set canonical error code 'response_body_not_available'",
    );
    t.equal(error.constructor.name, "Error", "should throw Error");
  }
});

test("webResponse() works after accessing body property", async (t) => {
  t.plan(3);

  try {
    const faithResponse = await fetch(url("/get"));

    // Access body property (creates stream but doesn't consume it)
    const bodyStream = faithResponse.body;
    t.ok(bodyStream, "should get body stream");

    // Should still be able to get webResponse()
    const webResponse = faithResponse.webResponse();
    t.ok(
      webResponse instanceof globalThis.Response,
      "should return Web API Response",
    );

    // Should be able to read from webResponse
    const text = await webResponse.text();
    t.ok(text, "should be able to read text from webResponse");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("webResponse() marks body as accessed", async (t) => {
  t.plan(2);

  try {
    const faithResponse = await fetch(url("/get"));

    // Get webResponse() first
    faithResponse.webResponse();

    // Try to consume body via text() - should fail
    await faithResponse.text();

    t.fail("Should have thrown error after webResponse()");
  } catch (error) {
    t.equal(
      error.message,
      native.errResponseAlreadyDisturbed(),
      "should throw 'Response already disturbed' error",
    );
    t.equal(
      error.code,
      native.errorCodes().response_already_disturbed,
      "should set canonical error code 'response_already_disturbed'",
    );
    t.equal(error.constructor.name, "Error", "should throw Error");
  }
});

test("webResponse() can be called multiple times", async (t) => {
  t.plan(4);

  try {
    const faithResponse = await fetch(url("/get"));

    // First call should work
    const webResponse1 = faithResponse.webResponse();
    t.ok(
      webResponse1 instanceof globalThis.Response,
      "first call should return Response",
    );

    // Second call should also work
    const webResponse2 = faithResponse.webResponse();
    t.ok(
      webResponse2 instanceof globalThis.Response,
      "second call should also return Response",
    );

    // They should be different Response objects
    t.notEqual(
      webResponse1,
      webResponse2,
      "should return different Response objects",
    );

    // Both should have the same properties
    t.equal(
      webResponse1.status,
      webResponse2.status,
      "both Responses should have same status",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("webResponse() returned Response has working json() method", async (t) => {
  t.plan(3);

  try {
    const faithResponse = await fetch(url("/get"));
    const webResponse = faithResponse.webResponse();

    // Use json() method on Web API Response
    const data = await webResponse.json();

    t.ok(data, "should get JSON data");
    t.ok(data.url, "JSON should have url property");
    t.ok(
      data.url.includes(new URL(url("/")).host + "/get"),
      "url should be correct",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("webResponse() with error response", async (t) => {
  t.plan(3);

  try {
    // Get a 404 response
    const faithResponse = await fetch(url("/status/404"));
    const webResponse = faithResponse.webResponse();

    t.equal(webResponse.status, 404, "status should be 404");
    t.equal(webResponse.ok, false, "ok should be false");
    t.equal(
      webResponse.statusText,
      faithResponse.statusText,
      "statusText should match",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("webResponse() body stream is shared", async (t) => {
  t.plan(5);

  try {
    const faithResponse = await fetch(url("/get"));

    // Get webResponse()
    const webResponse = faithResponse.webResponse();

    // Faith response body should still be available
    t.ok(
      faithResponse.body,
      "faithResponse.body should still be available after webResponse()",
    );

    // Faith response bodyUsed should be true
    t.equal(
      faithResponse.bodyUsed,
      true,
      "faithResponse.bodyUsed should be true after webResponse()",
    );

    // Web response should have body
    t.ok(webResponse.body, "webResponse should have body");

    // Should be able to read from webResponse body
    const reader = webResponse.body.getReader();
    const result = await reader.read();
    t.ok(!result.done, "should be able to read from webResponse body stream");

    // After reading from webResponse, faithResponse.body should still exist
    // but the stream might be locked
    t.ok(
      faithResponse.body,
      "faithResponse.body should still exist after reading from webResponse",
    );
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
