const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");

test("Test response.text() method", async (t) => {
  t.plan(3);

  const response = await faithFetch("https://httpbin.org/get");
  const text = await response.text();

  t.ok(typeof text === "string", "body should be convertible to string");
  t.ok(text.length > 0, "body should contain data");
  t.ok(text.includes("httpbin.org"), "body should contain response data");
});
