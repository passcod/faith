// Example demonstrating the faith fetch API with ReadableStream support

const { fetch } = require("../wrapper.js");

async function main() {
  console.log("=== Testing faith fetch with ReadableStream ===\n");

  try {
    // Make a request
    const response = await fetch("https://httpbin.org/get");

    console.log("Response status:", response.status);
    console.log("Response OK:", response.ok);
    console.log("Response URL:", response.url);
    console.log("Response headers:", response.headers);
    console.log("Response timestamp:", response.timestamp);
    console.log("Response bodyUsed:", response.bodyUsed);

    // Test 1: Get body as ReadableStream (property, not method)
    console.log("\n--- Test 1: Reading body as ReadableStream (property) ---");
    const stream = response.body;

    if (stream) {
      const reader = stream.getReader();
      let chunks = [];
      let totalBytes = 0;

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        chunks.push(value);
        totalBytes += value.length;
        console.log(`Read chunk of ${value.length} bytes`);
      }

      reader.releaseLock();

      // Combine chunks
      const combined = Buffer.concat(chunks);
      console.log(`Total bytes read: ${totalBytes}`);
      console.log(
        "First 100 chars of response:",
        combined.toString("utf-8").substring(0, 100) + "...",
      );

      // Parse as JSON to verify
      const json = JSON.parse(combined.toString("utf-8"));
      console.log("Successfully parsed as JSON");
      console.log("URL from JSON:", json.url);
    } else {
      console.log("No stream available (response already disturbed)");
    }

    // Test 2: Try to consume response again (should fail)
    console.log("\n--- Test 2: Trying to consume response again ---");
    const stream2 = response.body;
    console.log("Second body access returned:", stream2 ? "stream" : "null");

    if (!stream2) {
      console.log("Good! body returns null after first consumption");
      console.log("bodyUsed is now:", response.bodyUsed);
    }

    // Test 3: Make another request and use text() method
    console.log("\n--- Test 3: Using text() method ---");
    const response2 = await fetch("https://httpbin.org/get");
    const text = await response2.text();
    console.log("text() returned string of length:", text.length);
    console.log("First 100 chars:", text.substring(0, 100) + "...");

    // Test 4: Make another request and use bytes() method
    console.log("\n--- Test 4: Using bytes() method ---");
    const response3 = await fetch("https://httpbin.org/get");
    const bytes = await response3.bytes();
    console.log("bytes() returned array of length:", bytes.length);
    console.log("First 10 bytes:", bytes.slice(0, 10));

    // Test 5: Demonstrate that body, text(), and bytes() are mutually exclusive
    console.log("\n--- Test 5: Mutual exclusivity test ---");
    const response4 = await fetch("https://httpbin.org/get");

    // Get the stream first
    const stream4 = response4.body;
    if (stream4) {
      console.log("Got stream from body property");

      // Now try to use text() - should fail
      try {
        await response4.text();
        console.log("ERROR: text() should have thrown after body was called");
      } catch (err) {
        console.log("Good! text() threw error:", err.message);
        console.log("bodyUsed is now:", response4.bodyUsed);
      }

      // Try bytes() - should also fail
      try {
        await response4.bytes();
        console.log(
          "ERROR: bytes() should have thrown after body property was accessed",
        );
      } catch (err) {
        console.log("Good! bytes() threw error:", err.message);
        console.log("bodyUsed is still:", response4.bodyUsed);
      }
    }

    // Test 6: Demonstrate body_used getter
    console.log("\n--- Test 6: Testing body_used getter ---");
    const response5 = await fetch("https://httpbin.org/get");
    console.log("Initial bodyUsed:", response5.bodyUsed);

    const text5 = await response5.text();
    console.log("After text(), bodyUsed:", response5.bodyUsed);

    const response6 = await fetch("https://httpbin.org/get");
    console.log("Initial bodyUsed:", response6.bodyUsed);

    const stream6 = response6.body;
    if (stream6) {
      console.log(
        "After accessing body property, bodyUsed:",
        response6.bodyUsed,
      );
    }

    console.log("\n=== All tests completed successfully ===");
  } catch (error) {
    console.error("Error:", error);
  }
}

// Run the example
if (require.main === module) {
  main();
}

module.exports = { main };
