const { fetch, FetchClient } = require("../js/index.js");
const assert = require("assert");

async function testBasicFetch() {
  console.log("Testing basic fetch...");

  try {
    const response = await fetch("https://httpbin.org/get");
    assert.strictEqual(response.status, 200);
    assert.strictEqual(response.ok, true);
    assert(response.url.includes("httpbin.org"));

    const json = response.json();
    assert(json.url === "https://httpbin.org/get");

    console.log("✓ Basic fetch test passed");
  } catch (error) {
    console.error("✗ Basic fetch test failed:", error.message);
    throw error;
  }
}

async function testFetchClient() {
  console.log("Testing FetchClient...");

  try {
    const client = new FetchClient();

    const response = await client.get("https://httpbin.org/get");
    assert.strictEqual(response.status, 200);

    const postResponse = await client.post(
      "https://httpbin.org/post",
      JSON.stringify({ test: "data" }),
      {
        "Content-Type": "application/json",
      },
    );
    assert.strictEqual(postResponse.status, 200);
    const postJson = postResponse.json();
    assert(postJson.json.test === "data");

    console.log("✓ FetchClient test passed");
  } catch (error) {
    console.error("✗ FetchClient test failed:", error.message);
    throw error;
  }
}

async function testHeaders() {
  console.log("Testing headers...");

  try {
    const response = await fetch("https://httpbin.org/headers", {
      headers: {
        "X-Custom-Header": "test-value",
        "User-Agent": "faith-test/1.0",
      },
    });

    const json = response.json();
    assert(json.headers["X-Custom-Header"] === "test-value");
    assert(json.headers["User-Agent"] === "faith-test/1.0");

    console.log("✓ Headers test passed");
  } catch (error) {
    console.error("✗ Headers test failed:", error.message);
    throw error;
  }
}

async function testPostWithBody() {
  console.log("Testing POST with body...");

  try {
    const testData = { message: "Hello from faith", number: 42 };

    const response = await fetch("https://httpbin.org/post", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(testData),
    });

    const json = response.json();
    assert.deepStrictEqual(json.json, testData);

    console.log("✓ POST with body test passed");
  } catch (error) {
    console.error("✗ POST with body test failed:", error.message);
    throw error;
  }
}

async function runTest(test) {
  try {
    await test();
    return 0;
  } catch (_) {
    return 1;
  }
}

async function runTests() {
  console.log("Starting faith tests...\n");

  let nFailed = 0;
  nFailed += await runTest(testBasicFetch);
  nFailed += await runTest(testFetchClient);
  nFailed += await runTest(testHeaders);
  nFailed += await runTest(testPostWithBody);

  if (nFailed === 0) {
    console.log("\n✅ All tests passed!");
  } else {
    console.error(`\n❌ ${nFailed} tests failed`);
    process.exit(nFailed);
  }
}

if (require.main === module) {
  runTests();
}

module.exports = {
  testBasicFetch,
  testFetchClient,
  testHeaders,
  testPostWithBody,
  runTests,
};
