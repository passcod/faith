const test = require("tape");
const { compareResponses, hasNativeFetch } = require("./helpers.js");

test("Compare GET with query params", { skip: !hasNativeFetch }, async (t) => {
  await compareResponses(t, "/get?foo=bar&baz=qux");
});
