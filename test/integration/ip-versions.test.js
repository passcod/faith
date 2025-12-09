const test = require("tape");
const { fetch: faithFetch } = require("../../wrapper.js");

test("IPv4 - with test-ipv6.com endpoint", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://ipv4.lookup.test-ipv6.com");
  t.ok(response.ok, "Should successfully fetch from IPv4 test site");
  t.equal(response.status, 200, "Status should be 200");
});

test("IPv6 - with test-ipv6.com endpoint", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://ipv6.lookup.test-ipv6.com");
  t.ok(response.ok, "Should successfully fetch from IPv6 test site");
  t.equal(response.status, 200, "Status should be 200");
});
