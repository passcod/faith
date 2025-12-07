/**
 * Basic Usage Example for Faith Fetch API
 *
 * This example demonstrates the basic usage of the faith fetch library,
 * which provides a spec-compliant Fetch API implementation powered by Rust.
 */

const { fetch } = require("../wrapper.js");

async function main() {
  console.log("=== Faith Fetch API Basic Usage ===\n");

  try {
    // Example 1: Simple GET request
    console.log("--- Example 1: Simple GET request ---");
    const response1 = await fetch("https://httpbin.org/get");

    console.log("Status:", response1.status);
    console.log("OK:", response1.ok);
    console.log("URL:", response1.url);
    console.log("Headers:", response1.headers);
    console.log("Body used:", response1.bodyUsed);

    // Get response as text
    const text = await response1.text();
    console.log(
      "Response text (first 100 chars):",
      text.substring(0, 100) + "...",
    );

    // Example 2: GET with query parameters
    console.log("\n--- Example 2: GET with query parameters ---");
    const response2 = await fetch(
      "https://httpbin.org/get?name=faith&type=fetch",
    );
    const text2 = await response2.text();
    const json = JSON.parse(text2);
    console.log("Query args:", json.args);

    // Example 3: POST request with JSON body
    console.log("\n--- Example 3: POST request with JSON body ---");
    const postData = { message: "Hello from faith", number: 42 };
    const response3 = await fetch("https://httpbin.org/post", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(postData),
    });

    const postResultText = await response3.text();
    const postResult = JSON.parse(postResultText);
    console.log("Posted data:", postResult.json);
    console.log("Response status:", postResult.status);

    // Example 4: Using ReadableStream from body property
    console.log("\n--- Example 4: Using ReadableStream from body property ---");
    const response4 = await fetch("https://httpbin.org/get");

    // body is a property (getter), not a method
    const stream = response4.body;
    if (stream) {
      console.log("Got ReadableStream from response.body property");

      // Read from the stream
      const reader = stream.getReader();
      const chunks = [];

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks.push(value);
      }

      reader.releaseLock();

      const combined = Buffer.concat(chunks);
      console.log("Streamed", combined.length, "bytes");
      console.log(
        "Data (first 80 chars):",
        combined.toString("utf-8").substring(0, 80) + "...",
      );
    }

    // Example 5: Different ways to access response body
    console.log("\n--- Example 5: Different ways to access response body ---");

    // text() method
    const response5a = await fetch("https://httpbin.org/get");
    const textBody = await response5a.text();
    console.log(
      "text() returns:",
      typeof textBody,
      "of length",
      textBody.length,
    );

    // bytes() method (returns Uint8Array)
    const response5b = await fetch("https://httpbin.org/get");
    const bytes = await response5b.bytes();
    console.log(
      "bytes() returns:",
      bytes.constructor.name,
      "of length",
      bytes.length,
    );

    // arrayBuffer() method
    const response5c = await fetch("https://httpbin.org/get");
    const arrayBuffer = await response5c.arrayBuffer();
    console.log(
      "arrayBuffer() returns:",
      arrayBuffer.constructor.name,
      "of length",
      arrayBuffer.byteLength,
    );

    // Manual JSON parsing
    const response5d = await fetch("https://httpbin.org/get");
    const jsonText = await response5d.text();
    const jsonData = JSON.parse(jsonText);
    console.log(
      "JSON parsing returns:",
      typeof jsonData,
      "with URL:",
      jsonData.url,
    );

    // json() method
    const response5e = await fetch("https://httpbin.org/get");
    const jsonFromMethod = await response5e.json();
    console.log(
      "json() method returns:",
      typeof jsonFromMethod,
      "with URL:",
      jsonFromMethod.url,
    );

    // Example 6: Error handling
    console.log("\n--- Example 6: Error handling ---");
    try {
      await fetch("https://invalid-domain-that-does-not-exist-12345.com/");
    } catch (error) {
      console.log("Caught error for invalid URL:", error.message);
    }

    // Example 7: Timeout
    console.log("\n--- Example 7: Request with timeout ---");
    try {
      // This request will timeout before the 2-second delay completes
      await fetch("https://httpbin.org/delay/2", {
        timeout: 1, // 1 second timeout
      });
    } catch (error) {
      console.log("Request timed out as expected:", error.message);
    }

    console.log("\n=== All examples completed successfully ===");
  } catch (error) {
    console.error("Error:", error);
  }
}

// Run the example if this file is executed directly
if (require.main === module) {
  main();
}

module.exports = { main };
