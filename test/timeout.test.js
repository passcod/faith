const test = require("tape");
const { compareResponses, hasNativeFetch } = require("./helpers.js");

test("Compare request with timeout", { skip: !hasNativeFetch }, async (t) => {
  // Use a short timeout
  try {
    await compareResponses(t, "https://httpbin.org/delay/1", {
      timeout: 0.5, // 500ms timeout for a 1-second delay
    });
  } catch (error) {
    // Both should timeout
    t.pass("Request should timeout");
  }
});
