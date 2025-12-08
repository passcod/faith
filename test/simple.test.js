const test = require("tape");
const { fetch } = require("../wrapper.js");
const native = require("../index.js");
const { url, hostname } = require("./helpers.js");

test("simple: basic fetch works", async (t) => {
  t.plan(3);

  try {
    const response = await fetch(url("/get"));
    t.equal(response.status, 200, "should return status 200");
    t.ok(response.ok, "ok should be true for 200 status");
    t.ok(response.url.includes(hostname()), "url should contain hostname");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("simple: response.text() works", async (t) => {
  t.plan(2);

  try {
    const response = await fetch(url("/get"));
    const text = await response.text();
    t.ok(text, "should get text response");
    t.ok(text.includes(hostname()), "text should contain hostname");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("simple: response.json() works", async (t) => {
  t.plan(2);

  try {
    const response = await fetch(url("/get"));
    const json = await response.json();
    t.ok(json, "should get JSON response");
    t.ok(json.url, "JSON should have url property");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("simple: response.clone() works", async (t) => {
  t.plan(4);

  try {
    const response1 = await fetch(url("/get"));
    const response2 = response1.clone();

    t.notEqual(response1, response2, "clone should be different object");
    t.equal(response2.status, response1.status, "status should match");

    const text1 = await response1.text();
    const text2 = await response2.text();

    t.ok(text1, "original should read text");
    t.ok(text2, "clone should read text");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("simple: bodyUsed flag", async (t) => {
  t.plan(6);

  try {
    const response = await fetch(url("/get"));
    t.equal(response.bodyUsed, false, "bodyUsed should be false initially");

    await response.text();
    t.equal(response.bodyUsed, true, "bodyUsed should be true after reading");

    // Try to read again - should fail
    try {
      await response.text();
      t.fail("should have thrown error when reading disturbed body");
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
    }

    // Try to clone after reading - should fail
    try {
      response.clone();
      t.fail("should have thrown error when cloning disturbed body");
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
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("fetch() rejects invalid URL", async (t) => {
  t.plan(2);

  try {
    // Invalid URL (spaces) - should be rejected by native URL parsing
    await fetch("http://example .com");
    t.fail("Should have thrown TypeError for invalid URL");
  } catch (error) {
    t.equal(
      error.constructor.name,
      "TypeError",
      "should throw TypeError for invalid URL",
    );
    t.equal(
      error.code,
      native.errorCodes().invalid_url,
      "should set canonical error code 'invalid_url'",
    );
  }
});

test("fetch() rejects URL with credentials", async (t) => {
  t.plan(2);

  try {
    // URL with credentials should be rejected by the native implementation
    await fetch("https://user:pass@httpbin.org/get");
    t.fail("Should have thrown TypeError for credentials in URL");
  } catch (error) {
    t.equal(
      error.constructor.name,
      "TypeError",
      "should throw TypeError for credentials in URL",
    );
    t.equal(
      error.code,
      native.errorCodes().invalid_credentials,
      "should set canonical error code 'invalid_credentials'",
    );
  }
});
