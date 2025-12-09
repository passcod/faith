const test = require("tape");
const { fetch: faithFetch } = require("../../wrapper.js");

test("badssl.com - valid certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://badssl.com/");
  t.ok(response.ok, "Should successfully fetch from valid badssl.com");
  t.equal(response.status, 200, "Status should be 200");
});

test("badssl.com - expired certificate should fail", async (t) => {
  t.plan(1);

  try {
    await faithFetch("https://expired.badssl.com/");
    t.fail("Should throw for expired certificate");
  } catch (err) {
    t.pass("Should throw error for expired certificate");
  }
});

test("badssl.com - wrong host certificate should fail", async (t) => {
  t.plan(1);

  try {
    await faithFetch("https://wrong.host.badssl.com/");
    t.fail("Should throw for wrong host certificate");
  } catch (err) {
    t.pass("Should throw error for wrong host certificate");
  }
});

test("badssl.com - self-signed certificate should fail", async (t) => {
  t.plan(1);

  try {
    await faithFetch("https://self-signed.badssl.com/");
    t.fail("Should throw for self-signed certificate");
  } catch (err) {
    t.pass("Should throw error for self-signed certificate");
  }
});

test("badssl.com - untrusted root certificate should fail", async (t) => {
  t.plan(1);

  try {
    await faithFetch("https://untrusted-root.badssl.com/");
    t.fail("Should throw for untrusted root certificate");
  } catch (err) {
    t.pass("Should throw error for untrusted root certificate");
  }
});

// SKIP: we don't yet load custom CRLs — waiting for upki project from Canonical
test.skip("badssl.com - revoked certificate should fail", async (t) => {
  t.plan(1);

  try {
    await faithFetch("https://revoked.badssl.com/");
    t.fail("Should throw for revoked certificate");
  } catch (err) {
    t.pass("Should throw error for revoked certificate");
  }
});

test("badssl.com - incomplete chain should fail", async (t) => {
  t.plan(1);

  try {
    await faithFetch("https://incomplete-chain.badssl.com/");
    t.fail("Should throw for incomplete certificate chain");
  } catch (err) {
    t.pass("Should throw error for incomplete certificate chain");
  }
});

test("badssl.com - SHA-256 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://sha256.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with SHA-256 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

// SKIP: rustls doesn't support those — they don't seem to be in great use anyway
test.skip("badssl.com - SHA-384 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://sha384.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with SHA-384 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

// SKIP: the certificate here expired in 2022, so it's not testing that we support SHA-512
test.skip("badssl.com - SHA-512 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://sha512.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with SHA-512 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

// SKIP: the certificate here expired in 2021, so it's not testing that we support 1000 sans anyway
test.skip("badssl.com - 1000 subdomains certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://1000-sans.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with 1000 SANs certificate");
  t.equal(response.status, 200, "Status should be 200");
});

// SKIP: the handshake fails in Firefox, so we're pretty safe ignoring this one
test.skip("badssl.com - 10000 subdomains certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://10000-sans.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with 10000 SANs certificate");
  t.equal(response.status, 200, "Status should be 200");
});

test("badssl.com - ECC 256 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://ecc256.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with ECC 256 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

test("badssl.com - ECC 384 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://ecc384.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with ECC 384 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

test("badssl.com - RSA 2048 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://rsa2048.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with RSA 2048 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

test("badssl.com - RSA 4096 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://rsa4096.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with RSA 4096 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

// SKIP: the certificate here expired in 2024, so it's not testing that we support RSA 8192
test.skip("badssl.com - RSA 8192 certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://rsa8192.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with RSA 8192 certificate");
  t.equal(response.status, 200, "Status should be 200");
});

// SKIP: this certificate expired in 2022 — it's not EV failing, it's good old expiry
test.skip("badssl.com - extended validation certificate should succeed", async (t) => {
  t.plan(2);

  const response = await faithFetch("https://extended-validation.badssl.com/");
  t.ok(response.ok, "Should successfully fetch with EV certificate");
  t.equal(response.status, 200, "Status should be 200");
});
