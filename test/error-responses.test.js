const { url } = require("./helpers.js");
const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");
const { hasNativeFetch } = require("./helpers.js");

test("Compare error responses", { skip: !hasNativeFetch }, async (t) => {
  t.plan(2);

  // Test with invalid URL (should fail for both)
  let faithError = null;
  let nativeError = null;

  try {
    await faithFetch("https://invalid-domain-that-does-not-exist-12345.com/");
  } catch (error) {
    faithError = error;
  }

  try {
    await globalThis.fetch("https://invalid-domain-that-does-not-exist-12345.com/");
  } catch (error) {
    nativeError = error;
  }

  t.ok(faithError, "Faith should throw error for invalid URL");
  t.ok(nativeError, "Native fetch should throw error for invalid URL");
});
