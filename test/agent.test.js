const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Agent can be created", (t) => {
  t.plan(1);

  const agent = new Agent();
  t.ok(agent, "Agent should be created successfully");
});

test("Agent can be passed in options", async (t) => {
  t.plan(3);

  const agent = new Agent();
  const response = await faithFetch(url("/headers"), { agent });

  t.ok(response.ok, "Should successfully fetch with agent in options");
  const data = await response.json();
  const userAgent = data.headers["User-Agent"];

  t.ok(userAgent, "User-Agent header should be present");
  t.match(
    userAgent,
    /^Faith\/\d+\.\d+\.\d+ reqwest\/\d+\.\d+\.\d+$/,
    "User-Agent should match pattern when using Agent",
  );
});

test("Multiple requests can share the same Agent", async (t) => {
  t.plan(4);

  const agent = new Agent();

  const response1 = await faithFetch(url("/headers"), { agent });
  t.ok(response1.ok, "First request should succeed");

  const response2 = await faithFetch(url("/get"), { agent });
  t.ok(response2.ok, "Second request should succeed");

  const data1 = await response1.json();
  const data2 = await response2.json();

  t.ok(data1.headers["User-Agent"], "First request should have User-Agent");
  t.ok(data2.headers["User-Agent"], "Second request should have User-Agent");
});

test("Requests work without agent (creates default)", async (t) => {
  t.plan(3);

  const response = await faithFetch(url("/headers"));
  t.ok(response.ok, "Should successfully fetch without explicit agent");

  const data = await response.json();
  const userAgent = data.headers["User-Agent"];

  t.ok(userAgent, "User-Agent header should be present");
  t.match(
    userAgent,
    /^Faith\/\d+\.\d+\.\d+ reqwest\/\d+\.\d+\.\d+$/,
    "User-Agent should match pattern with default agent",
  );
});

test("Agent can be used with other options", async (t) => {
  t.plan(4);

  const agent = new Agent();
  const response = await faithFetch(url("/post"), {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-Custom-Header": "test-value",
    },
    body: JSON.stringify({ test: "data" }),
    agent,
  });

  t.ok(response.ok, "Should successfully POST with agent and other options");

  const data = await response.json();
  t.ok(data.headers["User-Agent"], "Should have User-Agent");
  t.equal(
    data.headers["X-Custom-Header"],
    "test-value",
    "Custom header should be sent",
  );
  t.equal(data.json.test, "data", "POST body should be received");
});

test("Different agents can be used for different requests", async (t) => {
  t.plan(4);

  const agent1 = new Agent();
  const agent2 = new Agent();

  const response1 = await faithFetch(url("/headers"), { agent: agent1 });
  const response2 = await faithFetch(url("/headers"), { agent: agent2 });

  t.ok(response1.ok, "First request should succeed");
  t.ok(response2.ok, "Second request should succeed");

  const data1 = await response1.json();
  const data2 = await response2.json();

  t.ok(data1.headers["User-Agent"], "First request should have User-Agent");
  t.ok(data2.headers["User-Agent"], "Second request should have User-Agent");
});

test("Agent works with timeout option", async (t) => {
  t.plan(2);

  const agent = new Agent();
  const response = await faithFetch(url("/delay/1"), {
    agent,
    timeout: 5,
  });

  t.ok(response.ok, "Should successfully fetch with agent and timeout");
  t.equal(response.status, 200, "Status should be 200");
});

test("Agent persists across async operations", async (t) => {
  t.plan(6);

  const agent = new Agent();

  const promises = [
    faithFetch(url("/get"), { agent }),
    faithFetch(url("/headers"), { agent }),
    faithFetch(url("/status/200"), { agent }),
  ];

  const responses = await Promise.all(promises);

  t.ok(responses[0].ok, "First parallel request should succeed");
  t.ok(responses[1].ok, "Second parallel request should succeed");
  t.ok(responses[2].ok, "Third parallel request should succeed");

  const data1 = await responses[0].json();
  const data2 = await responses[1].json();

  t.ok(data1.headers["User-Agent"], "First request should have User-Agent");
  t.ok(data2.headers["User-Agent"], "Second request should have User-Agent");
  t.equal(responses[2].status, 200, "Third request should have status 200");
});

test("Agent in options doesn't interfere with custom User-Agent header", async (t) => {
  t.plan(2);

  const agent = new Agent();
  const customUserAgent = "CustomClient/1.0";
  const response = await faithFetch(url("/headers"), {
    agent,
    headers: {
      "User-Agent": customUserAgent,
    },
  });

  t.ok(
    response.ok,
    "Should successfully fetch with agent and custom User-Agent",
  );

  const data = await response.json();
  t.equal(
    data.headers["User-Agent"],
    customUserAgent,
    "Custom User-Agent should override agent default",
  );
});
