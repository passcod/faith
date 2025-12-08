const test = require("tape");
const { compareResponses, hasNativeFetch } = require("./helpers.js");

test("Compare different HTTP methods", { skip: !hasNativeFetch }, async (t) => {
  const methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];

  for (const method of methods) {
    const path = `/${method.toLowerCase()}`;
    const options = { method };

    if (method === "POST" || method === "PUT" || method === "PATCH") {
      options.body = Array.from(Buffer.from("test body"));
    }

    await compareResponses(t, path, options);
  }
});
