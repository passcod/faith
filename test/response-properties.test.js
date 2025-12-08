const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Test response properties", async (t) => {
  t.plan(8);

  const response = await faithFetch(url("/get"));

  t.equal(typeof response.status, "number", "status should be a number");
  t.equal(
    typeof response.statusText,
    "string",
    "statusText should be a string",
  );
  t.equal(typeof response.ok, "boolean", "ok should be a boolean");
  t.equal(typeof response.url, "string", "url should be a string");
  t.equal(
    typeof response.redirected,
    "boolean",
    "redirected should be a boolean",
  );
  t.equal(typeof response.timestamp, "number", "timestamp should be a number");
  t.equal(typeof response.bodyUsed, "boolean", "bodyUsed should be a boolean");
  t.equal(
    typeof response.body,
    "object",
    "body should be an object (property)",
  );
});
