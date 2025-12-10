const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("User-Agent header should be set to Faith/version reqwest/version", async (t) => {
  t.plan(3);

  const response = await faithFetch(url("/headers"));
  t.ok(response.ok, "Should successfully fetch");

  const data = await response.json();
  const userAgent = data.headers["User-Agent"];

  t.ok(userAgent, "User-Agent header should be present");
  t.match(
    userAgent,
    /^Faith\/\d+\.\d+\.\d+ reqwest\/\d+\.\d+\.\d+$/,
    "User-Agent should match pattern 'Faith/x.y.z reqwest/x.y.z'",
  );
});

test("User-Agent header should start with Faith/", async (t) => {
  t.plan(2);

  const response = await faithFetch(url("/headers"));
  const data = await response.json();
  const userAgent = data.headers["User-Agent"];

  t.ok(userAgent.startsWith("Faith/"), "User-Agent should start with 'Faith/'");
  t.ok(
    userAgent.includes(" reqwest/"),
    "User-Agent should include ' reqwest/'",
  );
});

test("User-Agent can be overridden with custom headers", async (t) => {
  t.plan(2);

  const customUserAgent = "CustomClient/1.0";
  const response = await faithFetch(url("/headers"), {
    headers: {
      "User-Agent": customUserAgent,
    },
  });

  t.ok(response.ok, "Should successfully fetch with custom User-Agent");

  const data = await response.json();
  t.equal(
    data.headers["User-Agent"],
    customUserAgent,
    "Custom User-Agent should override default",
  );
});

test("Agent with custom userAgent option", async (t) => {
  t.plan(2);

  const customUserAgent = "MyCustomAgent/2.0";
  const agent = new Agent({ userAgent: customUserAgent });
  const response = await faithFetch(url("/headers"), { agent });

  t.ok(response.ok, "Should successfully fetch with custom userAgent in Agent");

  const data = await response.json();
  t.equal(
    data.headers["User-Agent"],
    customUserAgent,
    "Agent userAgent option should set User-Agent header",
  );
});

test("Agent userAgent persists across multiple requests", async (t) => {
  t.plan(5);

  const customUserAgent = "PersistentAgent/1.0";
  const agent = new Agent({ userAgent: customUserAgent });

  const response1 = await faithFetch(url("/headers"), { agent });
  const response2 = await faithFetch(url("/get"), { agent });

  t.ok(response1.ok, "First request should succeed");
  t.ok(response2.ok, "Second request should succeed");

  const data1 = await response1.json();
  const data2 = await response2.json();

  t.equal(
    data1.headers["User-Agent"],
    customUserAgent,
    "First request should use custom User-Agent from Agent",
  );
  t.equal(
    data2.headers["User-Agent"],
    customUserAgent,
    "Second request should use custom User-Agent from Agent",
  );
  t.equal(
    data1.headers["User-Agent"],
    data2.headers["User-Agent"],
    "Both requests should have the same User-Agent",
  );
});

test("Request-level User-Agent header overrides Agent userAgent option", async (t) => {
  t.plan(2);

  const agentUserAgent = "AgentUA/1.0";
  const requestUserAgent = "RequestUA/2.0";
  const agent = new Agent({ userAgent: agentUserAgent });

  const response = await faithFetch(url("/headers"), {
    agent,
    headers: {
      "User-Agent": requestUserAgent,
    },
  });

  t.ok(
    response.ok,
    "Should successfully fetch with both Agent and request User-Agent",
  );

  const data = await response.json();
  t.equal(
    data.headers["User-Agent"],
    requestUserAgent,
    "Request-level User-Agent should override Agent userAgent option",
  );
});

test("Different Agents can have different userAgent values", async (t) => {
  t.plan(4);

  const agent1 = new Agent({ userAgent: "FirstAgent/1.0" });
  const agent2 = new Agent({ userAgent: "SecondAgent/2.0" });

  const response1 = await faithFetch(url("/headers"), { agent: agent1 });
  const response2 = await faithFetch(url("/headers"), { agent: agent2 });

  t.ok(response1.ok, "First request should succeed");
  t.ok(response2.ok, "Second request should succeed");

  const data1 = await response1.json();
  const data2 = await response2.json();

  t.equal(
    data1.headers["User-Agent"],
    "FirstAgent/1.0",
    "First Agent should use its custom User-Agent",
  );
  t.equal(
    data2.headers["User-Agent"],
    "SecondAgent/2.0",
    "Second Agent should use its custom User-Agent",
  );
});

test("Agent without userAgent option uses default User-Agent", async (t) => {
  t.plan(3);

  const agent = new Agent();
  const response = await faithFetch(url("/headers"), { agent });

  t.ok(response.ok, "Should successfully fetch with Agent without userAgent");

  const data = await response.json();
  const userAgent = data.headers["User-Agent"];

  t.ok(userAgent, "User-Agent header should be present");
  t.match(
    userAgent,
    /^Faith\/\d+\.\d+\.\d+ reqwest\/\d+\.\d+\.\d+$/,
    "Should use default Faith/reqwest User-Agent pattern",
  );
});

test("Agent with empty userAgent option", async (t) => {
  t.plan(2);

  const agent = new Agent({ userAgent: "" });
  const response = await faithFetch(url("/headers"), { agent });

  t.ok(response.ok, "Should successfully fetch with empty userAgent");

  const data = await response.json();
  t.equal(
    data.headers["User-Agent"],
    "",
    "Empty userAgent should set empty User-Agent header",
  );
});

test("Agent userAgent with special characters", async (t) => {
  t.plan(2);

  const customUserAgent = "MyAgent/1.0 (Linux; x86_64) Custom/2.0";
  const agent = new Agent({ userAgent: customUserAgent });
  const response = await faithFetch(url("/headers"), { agent });

  t.ok(
    response.ok,
    "Should successfully fetch with special characters in userAgent",
  );

  const data = await response.json();
  t.equal(
    data.headers["User-Agent"],
    customUserAgent,
    "Special characters in userAgent should be preserved",
  );
});
