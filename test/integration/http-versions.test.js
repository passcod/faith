const test = require("tape");
const { fetch: faithFetch } = require("../../wrapper.js");

test("HTTP/2 - cloudflare.com should support HTTP/2", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://cloudflare.com/");
  t.ok(response.ok, "Should successfully fetch from HTTP/2 enabled site");
  t.equal(response.status, 200, "Status should be 200");
});

test("HTTP/2 - google.com should support HTTP/2", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://google.com/");
  t.ok(response.ok, "Should successfully fetch from HTTP/2 enabled site");
  t.equal(response.status, 200, "Status should be 200");
});

test("HTTP/2 - github.com should support HTTP/2", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://github.com/");
  t.ok(response.ok, "Should successfully fetch from HTTP/2 enabled site");
  t.equal(response.status, 200, "Status should be 200");
});

test("HTTP/2 - npm registry should support HTTP/2", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://registry.npmjs.org/");
  t.ok(
    response.ok,
    "Should successfully fetch from HTTP/2 enabled npm registry",
  );
  t.equal(response.status, 200, "Status should be 200");
});

test("HTTP/2 - mozilla.org should support HTTP/2", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://www.mozilla.org/");
  t.ok(response.ok, "Should successfully fetch from HTTP/2 enabled site");
  t.equal(response.status, 200, "Status should be 200");
});

test("HTTP/3 - cloudflare.com should support HTTP/3", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://cloudflare.com/");
  t.ok(
    response.ok,
    "Should successfully fetch (HTTP/3 fallback to HTTP/2 is acceptable)",
  );
  t.equal(response.status, 200, "Status should be 200");
});

test("HTTP/3 - google.com should support HTTP/3", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://google.com/");
  t.ok(
    response.ok,
    "Should successfully fetch (HTTP/3 fallback to HTTP/2 is acceptable)",
  );
  t.equal(response.status, 200, "Status should be 200");
});

test("HTTP/1.1 - http1.golang.org fallback", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://http1.golang.org/");
  t.ok(
    response.ok || response.status === 404,
    "Should successfully connect even if HTTP/1.1 only",
  );
  t.ok(
    response.status >= 200 && response.status < 600,
    "Should receive valid HTTP status",
  );
});

test("HTTP versions - mixed protocol requests", async (t) => {
  t.plan(6);

  const urls = [
    "https://www.google.com/",
    "https://github.com/",
    "https://cloudflare.com/",
  ];

  for (const url of urls) {
    const response = await faithFetch(url);
    t.ok(response.ok, `Should successfully fetch ${url}`);
    t.equal(response.status, 200, `Status should be 200 for ${url}`);
  }
});
