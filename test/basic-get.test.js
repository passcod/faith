const test = require("tape");
const { compareResponses, hasNativeFetch } = require("./helpers.js");

test("Compare basic GET request", { skip: !hasNativeFetch }, async (t) => {
  // Don't set a fixed plan since compareResponses has variable assertions
  await compareResponses(t, "https://httpbin.org/get");
});
