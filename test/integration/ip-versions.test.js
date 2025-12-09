const test = require("tape");
const { fetch: faithFetch } = require("../../wrapper.js");

test("IPv6 - ipv6.google.com should be accessible via IPv6", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://ipv6.google.com/");
  t.ok(response.ok, "Should successfully fetch from IPv6-only domain");
  t.equal(response.status, 200, "Status should be 200");
});

test("IPv6 - test-ipv6.com should be accessible", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://test-ipv6.com/");
  t.ok(response.ok, "Should successfully fetch from IPv6 test site");
  t.equal(response.status, 200, "Status should be 200");
});

test("IPv6 - ipv6-test.com should be accessible", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://ipv6-test.com/");
  t.ok(response.ok, "Should successfully fetch from IPv6 test site");
  t.equal(response.status, 200, "Status should be 200");
});

test("IPv4 - cloudflare.com should be accessible via IPv4", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://cloudflare.com/");
  t.ok(response.ok, "Should successfully fetch from IPv4 domain");
  t.equal(response.status, 200, "Status should be 200");
});

test("IPv4 - example.com should be accessible via IPv4", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://example.com/");
  t.ok(response.ok, "Should successfully fetch from IPv4 domain");
  t.equal(response.status, 200, "Status should be 200");
});

test("Dual-stack - google.com should be accessible", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://google.com/");
  t.ok(response.ok, "Should successfully fetch from dual-stack domain");
  t.equal(response.status, 200, "Status should be 200");
});

test("Dual-stack - github.com should be accessible", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://github.com/");
  t.ok(response.ok, "Should successfully fetch from dual-stack domain");
  t.equal(response.status, 200, "Status should be 200");
});

test("Dual-stack - mozilla.org should be accessible", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://www.mozilla.org/");
  t.ok(response.ok, "Should successfully fetch from dual-stack domain");
  t.equal(response.status, 200, "Status should be 200");
});

test("IPv6 - multiple IPv6-enabled sites", async (t) => {
  t.plan(4);

  const ipv6Sites = ["https://www.google.com/", "https://www.facebook.com/"];

  for (const url of ipv6Sites) {
    const response = await faithFetch(url);
    t.ok(response.ok, `Should successfully fetch ${url}`);
    t.equal(response.status, 200, `Status should be 200 for ${url}`);
  }
});
