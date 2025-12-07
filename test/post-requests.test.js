const test = require("tape");
const { compareResponses, hasNativeFetch } = require("./helpers.js");

test(
  "Compare POST request with JSON body",
  { skip: !hasNativeFetch },
  async (t) => {
    const testData = { message: "Hello from faith", number: 42 };

    await compareResponses(t, "https://httpbin.org/post", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(testData),
    });
  },
);
