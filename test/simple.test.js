const test = require("tape");
const { fetch } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("simple: basic fetch works", async (t) => {
  t.plan(3);

  try {
    const response = await fetch(url("/get"));
    t.equal(response.status, 200, "should return status 200");
    t.ok(response.ok, "ok should be true for 200 status");
    t.ok(
      response.url.includes(new URL(url("/")).hostname),
      "url should contain ${hostname()}",
    );
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
    t.ok(
      text.includes(new URL(url("/")).hostname),
      "text should contain ${hostname()}",
    );
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
  t.plan(4);

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
      t.ok(
        error.message.includes("disturbed") ||
          error.message.includes("already"),
        "error should mention disturbed or already",
      );
    }

    // Try to clone after reading - should fail
    try {
      response.clone();
      t.fail("should have thrown error when cloning disturbed body");
    } catch (error) {
      t.ok(
        error.message.includes("disturbed") ||
          error.message.includes("already"),
        "error should mention disturbed or already",
      );
    }
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
