const test = require("tape");
const { compareResponses, hasNativeFetch, url } = require("./helpers.js");
const { fetch } = require("../wrapper.js");
const native = require("../index.js");

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

test(
  "fetch rejects invalid HTTP method",
  { skip: !hasNativeFetch },
  async (t) => {
    t.plan(1);

    try {
      await fetch(url("/get"), { method: "INV@LID-METHOD!" });
      t.fail("Should have thrown error when using invalid HTTP method");
    } catch (error) {
      t.equal(
        error.code,
        native.errorCodes().invalid_method,
        "should set canonical error code 'invalid_method'",
      );
    }
  },
);
