const test = require("tape");
const { compareResponses, hasNativeFetch } = require("./helpers.js");

test(
  "Compare request with custom headers",
  { skip: !hasNativeFetch },
  async (t) => {
    await compareResponses(t, "/headers", {
      headers: {
        "X-Custom-Header": "test-value",
      },
    });
  },
);
