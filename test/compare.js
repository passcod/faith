const { fetch: rustFetch } = require("../js/index.js");
const assert = require("assert");

async function compareFetch(url, options = {}) {
  console.log(`\nComparing fetch for: ${url}`);

  const startRust = performance.now();
  const rustResponse = await rustFetch(url, options);
  const rustTime = performance.now() - startRust;

  const startNative = performance.now();
  const nativeResponse = await globalThis.fetch(url, options);
  const nativeTime = performance.now() - startNative;

  const rustText = await rustResponse.text();
  const nativeText = await nativeResponse.text();

  const rustJson = rustResponse.json();
  const nativeJson = JSON.parse(nativeText);

  console.log(`Rust fetch time: ${rustTime.toFixed(2)}ms`);
  console.log(`Native fetch time: ${nativeTime.toFixed(2)}ms`);
  console.log(`Time difference: ${(rustTime - nativeTime).toFixed(2)}ms`);

  assert.strictEqual(
    rustResponse.status,
    nativeResponse.status,
    "Status codes should match",
  );
  assert.strictEqual(
    rustResponse.ok,
    nativeResponse.ok,
    "OK status should match",
  );
  assert.strictEqual(
    rustResponse.redirected,
    nativeResponse.redirected,
    "Redirect status should match",
  );
  assert.strictEqual(rustText, nativeText, "Response text should match");

  if (rustJson && nativeJson) {
    const rustKeys = Object.keys(rustJson).sort();
    const nativeKeys = Object.keys(nativeJson).sort();

    assert.deepStrictEqual(
      rustKeys,
      nativeKeys,
      "Response JSON keys should match",
    );

    for (const key of rustKeys) {
      if (typeof rustJson[key] === "object" && rustJson[key] !== null) {
        continue;
      }
      assert.strictEqual(
        String(rustJson[key]),
        String(nativeJson[key]),
        `Value for key "${key}" should match`,
      );
    }
  }

  return {
    rustTime,
    nativeTime,
    rustResponse,
    nativeResponse,
    match: true,
  };
}

async function compareHeaders() {
  console.log("\nComparing headers...");

  const url = "https://httpbin.org/headers";
  const customHeaders = {
    "X-Test-Header": "test-value",
    "User-Agent": "faith-comparison-test",
  };

  const rustResponse = await rustFetch(url, { headers: customHeaders });
  const nativeResponse = await globalThis.fetch(url, {
    headers: customHeaders,
  });

  const rustJson = rustResponse.json();
  const nativeText = await nativeResponse.text();
  const nativeJson = JSON.parse(nativeText);

  assert.strictEqual(
    rustJson.headers["X-Test-Header"],
    nativeJson.headers["X-Test-Header"],
    "Custom headers should match",
  );

  assert.strictEqual(
    rustJson.headers["User-Agent"],
    nativeJson.headers["User-Agent"],
    "User-Agent headers should match",
  );

  console.log("✓ Headers comparison passed");
}

async function comparePostRequest() {
  console.log("\nComparing POST requests...");

  const url = "https://httpbin.org/post";
  const testData = {
    timestamp: Date.now(),
    message: "Comparison test",
    values: [1, 2, 3, 4, 5],
  };

  const options = {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Request-ID": "test-" + Date.now(),
    },
    body: JSON.stringify(testData),
  };

  const rustResponse = await rustFetch(url, options);
  const nativeResponse = await globalThis.fetch(url, options);

  const rustJson = rustResponse.json();
  const nativeText = await nativeResponse.text();
  const nativeJson = JSON.parse(nativeText);

  assert.deepStrictEqual(
    rustJson.json,
    nativeJson.json,
    "POST body should match",
  );
  assert.strictEqual(
    rustJson.headers["Content-Type"],
    nativeJson.headers["Content-Type"],
    "Content-Type should match",
  );

  console.log("✓ POST request comparison passed");
}

async function compareErrorHandling() {
  console.log("\nComparing error handling...");

  const invalidUrl = "https://invalid-domain-that-does-not-exist-12345.com/";

  let rustError = null;
  let nativeError = null;

  try {
    await rustFetch(invalidUrl, { timeout: 5000 });
  } catch (error) {
    rustError = error;
  }

  try {
    await globalThis.fetch(invalidUrl, { signal: AbortSignal.timeout(5000) });
  } catch (error) {
    nativeError = error;
  }

  assert(rustError !== null, "Rust fetch should throw error for invalid URL");
  assert(
    nativeError !== null,
    "Native fetch should throw error for invalid URL",
  );

  console.log("✓ Error handling comparison passed");
}

async function benchmarkMultipleRequests() {
  console.log("\nBenchmarking multiple requests...");

  const urls = [
    "https://httpbin.org/get",
    "https://httpbin.org/headers",
    "https://httpbin.org/ip",
    "https://httpbin.org/user-agent",
  ];

  const rustTimes = [];
  const nativeTimes = [];

  for (const url of urls) {
    const startRust = performance.now();
    await rustFetch(url);
    rustTimes.push(performance.now() - startRust);

    const startNative = performance.now();
    await globalThis.fetch(url);
    nativeTimes.push(performance.now() - startNative);
  }

  const rustAvg = rustTimes.reduce((a, b) => a + b, 0) / rustTimes.length;
  const nativeAvg = nativeTimes.reduce((a, b) => a + b, 0) / nativeTimes.length;

  console.log(`Average Rust fetch time: ${rustAvg.toFixed(2)}ms`);
  console.log(`Average Native fetch time: ${nativeAvg.toFixed(2)}ms`);
  console.log(`Performance ratio: ${(rustAvg / nativeAvg).toFixed(2)}x`);

  return {
    rustAvg,
    nativeAvg,
    ratio: rustAvg / nativeAvg,
  };
}

async function runComparison() {
  console.log("Starting fetch comparison tests...");
  console.log("===============================\n");

  try {
    await compareFetch("https://httpbin.org/get");
    await compareFetch("https://httpbin.org/ip");
    await compareFetch("https://httpbin.org/user-agent");

    await compareHeaders();
    await comparePostRequest();
    await compareErrorHandling();

    const benchmark = await benchmarkMultipleRequests();

    console.log("\n===============================");
    console.log("Comparison Summary:");
    console.log(
      `Rust/Reqwest is ${benchmark.ratio.toFixed(2)}x slower than native fetch`,
    );

    if (benchmark.ratio < 2.0) {
      console.log("✅ Performance is acceptable (within 2x of native)");
    } else {
      console.log(
        "⚠️  Performance could be improved (more than 2x slower than native)",
      );
    }

    console.log("\n✅ All comparison tests passed!");
  } catch (error) {
    console.error("\n❌ Comparison test failed:", error.message);
    console.error(error.stack);
    process.exit(1);
  }
}

if (require.main === module) {
  if (typeof globalThis.fetch === "undefined") {
    console.error("Native fetch is not available in this Node.js version.");
    console.error("Please use Node.js 18 or later, or install node-fetch.");
    process.exit(1);
  }

  runComparison();
}

module.exports = {
  compareFetch,
  compareHeaders,
  comparePostRequest,
  compareErrorHandling,
  benchmarkMultipleRequests,
  runComparison,
};
