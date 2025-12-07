const { fetch, fetchJson, fetchText, FetchClient } = require("./js/index.js");

async function runExamples() {
  console.log("faith - Rust-powered fetch examples\n");

  try {
    console.log("1. Basic fetch:");
    const response = await fetch("https://httpbin.org/get");
    console.log(`   Status: ${response.status} ${response.statusText}`);
    console.log(`   URL: ${response.url}`);
    console.log(`   Timestamp: ${response.timestamp.toISOString()}`);

    const data = response.json();
    console.log(`   Response keys: ${Object.keys(data).join(", ")}`);
    console.log();

    console.log("2. Fetch with custom headers:");
    const headersResponse = await fetch("https://httpbin.org/headers", {
      headers: {
        "X-Custom-Header": "faith-example",
        "User-Agent": "faith/1.0",
      },
    });
    const headersData = headersResponse.json();
    console.log(`   Custom header: ${headersData.headers["X-Custom-Header"]}`);
    console.log(`   User-Agent: ${headersData.headers["User-Agent"]}`);
    console.log();

    console.log("3. POST request with JSON body:");
    const postResponse = await fetch("https://httpbin.org/post", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        message: "Hello from faith",
        timestamp: Date.now(),
      }),
    });
    const postData = postResponse.json();
    console.log(`   Status: ${postResponse.status}`);
    console.log(`   Echoed data: ${JSON.stringify(postData.json)}`);
    console.log();

    console.log("4. Using fetchJson helper:");
    const jsonData = await fetchJson("https://httpbin.org/ip");
    console.log(`   Your IP: ${jsonData.origin}`);
    console.log();

    console.log("5. Using fetchText helper:");
    const html = await fetchText("https://httpbin.org/html");
    console.log(`   HTML response length: ${html.length} characters`);
    console.log(`   First 100 chars: ${html.substring(0, 100)}...`);
    console.log();

    console.log("6. Using FetchClient class:");
    const client = new FetchClient();

    const clientResponse = await client.get("https://httpbin.org/get");
    console.log(`   Client GET status: ${clientResponse.status}`);

    const clientPostResponse = await client.post(
      "https://httpbin.org/post",
      JSON.stringify({ client: "example" }),
      { "Content-Type": "application/json" },
    );
    console.log(`   Client POST status: ${clientPostResponse.status}`);
    console.log();

    console.log("7. Error handling:");
    try {
      await fetch("https://invalid-url-that-does-not-exist-12345.com/");
    } catch (error) {
      console.log(`   Expected error: ${error.message}`);
    }
    console.log();

    console.log("✅ All examples completed successfully!");
  } catch (error) {
    console.error("❌ Example failed:", error.message);
    process.exit(1);
  }
}

if (require.main === module) {
  runExamples();
}

module.exports = { runExamples };
