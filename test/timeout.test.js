const { url } = require("./helpers.js");
const test = require("tape");
const { fetch } = require("../wrapper.js");

test("timeout: request times out when too short", async (t) => {
  t.plan(2);

  try {
    await fetch(url("/delay/2"), {
      timeout: 500, // 500ms timeout for 2-second delay
    });
    t.fail("Should have timed out");
  } catch (error) {
    t.ok(error, "should throw error");
    t.equal(error.code, "Timeout", "should be a timeout error");
  }
});

test("timeout: request completes within timeout", async (t) => {
  t.plan(1);

  try {
    const response = await fetch(url("/get"), {
      timeout: 5000, // 5000ms timeout
    });
    t.equal(response.status, 200, "should complete successfully");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("timeout: works with POST requests", async (t) => {
  t.plan(2);

  try {
    const response = await fetch(url("/post"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ test: "data" }),
      timeout: 5000,
    });
    t.equal(response.status, 200, "should complete POST request");

    const data = await response.json();
    t.equal(data.json.test, "data", "should send body correctly");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
