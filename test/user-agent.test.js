const test = require("tape");
const { fetch: faithFetch } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("User-Agent header should be set to Faith/version reqwest/version", async (t) => {
  t.plan(3);

  const response = await faithFetch(url("/headers"));
  t.ok(response.ok, "Should successfully fetch");

  const data = await response.json();
  const userAgent = data.headers["User-Agent"];

  t.ok(userAgent, "User-Agent header should be present");
  t.match(
    userAgent,
    /^Faith\/\d+\.\d+\.\d+ reqwest\/\d+\.\d+\.\d+$/,
    "User-Agent should match pattern 'Faith/x.y.z reqwest/x.y.z'",
  );
});

test("User-Agent header should start with Faith/", async (t) => {
  t.plan(2);

  const response = await faithFetch(url("/headers"));
  const data = await response.json();
  const userAgent = data.headers["User-Agent"];

  t.ok(userAgent.startsWith("Faith/"), "User-Agent should start with 'Faith/'");
  t.ok(
    userAgent.includes(" reqwest/"),
    "User-Agent should include ' reqwest/'",
  );
});

test("User-Agent can be overridden with custom headers", async (t) => {
  t.plan(2);

  const customUserAgent = "CustomClient/1.0";
  const response = await faithFetch(url("/headers"), {
    headers: {
      "User-Agent": customUserAgent,
    },
  });

  t.ok(response.ok, "Should successfully fetch with custom User-Agent");

  const data = await response.json();
  t.equal(
    data.headers["User-Agent"],
    customUserAgent,
    "Custom User-Agent should override default",
  );
});
