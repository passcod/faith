const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

test("Agent stats() returns initial values", (t) => {
  t.plan(3);

  const agent = new Agent();
  const stats = agent.stats();

  t.ok(stats, "stats() should return an object");
  t.equal(stats.requestsSent, 0, "requestsSent should be 0 initially");
  t.equal(
    stats.responsesReceived,
    0,
    "responsesReceived should be 0 initially",
  );
});

test("Agent stats() tracks single request", async (t) => {
  t.plan(3);

  const agent = new Agent();
  await faithFetch(url("/get"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1 after one request");
  t.equal(
    stats.responsesReceived,
    1,
    "responsesReceived should be 1 after one response",
  );
  t.equal(
    stats.requestsSent,
    stats.responsesReceived,
    "Counts should match for successful request",
  );
});

test("Agent stats() tracks multiple requests", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/get"), { agent });
  await faithFetch(url("/headers"), { agent });
  await faithFetch(url("/status/200"), { agent });

  const stats = agent.stats();
  t.equal(
    stats.requestsSent,
    3,
    "requestsSent should be 3 after three requests",
  );
  t.equal(
    stats.responsesReceived,
    3,
    "responsesReceived should be 3 after three responses",
  );
});

test("Agent stats() tracks parallel requests", async (t) => {
  t.plan(2);

  const agent = new Agent();

  const promises = [
    faithFetch(url("/get"), { agent }),
    faithFetch(url("/headers"), { agent }),
    faithFetch(url("/status/200"), { agent }),
    faithFetch(url("/status/201"), { agent }),
    faithFetch(url("/status/202"), { agent }),
  ];

  await Promise.all(promises);

  const stats = agent.stats();
  t.equal(
    stats.requestsSent,
    5,
    "requestsSent should be 5 after five parallel requests",
  );
  t.equal(
    stats.responsesReceived,
    5,
    "responsesReceived should be 5 after five parallel responses",
  );
});

test("Agent stats() increments across multiple calls", async (t) => {
  t.plan(6);

  const agent = new Agent();

  await faithFetch(url("/get"), { agent });
  let stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(stats.responsesReceived, 1, "responsesReceived should be 1");

  await faithFetch(url("/headers"), { agent });
  stats = agent.stats();
  t.equal(stats.requestsSent, 2, "requestsSent should be 2");
  t.equal(stats.responsesReceived, 2, "responsesReceived should be 2");

  await faithFetch(url("/status/200"), { agent });
  stats = agent.stats();
  t.equal(stats.requestsSent, 3, "requestsSent should be 3");
  t.equal(stats.responsesReceived, 3, "responsesReceived should be 3");
});

test("Different agents have separate stats", async (t) => {
  t.plan(4);

  const agent1 = new Agent();
  const agent2 = new Agent();

  await faithFetch(url("/get"), { agent: agent1 });
  await faithFetch(url("/headers"), { agent: agent1 });

  await faithFetch(url("/get"), { agent: agent2 });

  const stats1 = agent1.stats();
  const stats2 = agent2.stats();

  t.equal(stats1.requestsSent, 2, "agent1 should have 2 requests");
  t.equal(stats1.responsesReceived, 2, "agent1 should have 2 responses");
  t.equal(stats2.requestsSent, 1, "agent2 should have 1 request");
  t.equal(stats2.responsesReceived, 1, "agent2 should have 1 response");
});

test("Agent stats() tracks POST requests", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/post"), {
    method: "POST",
    body: JSON.stringify({ test: "data" }),
    headers: { "Content-Type": "application/json" },
    agent,
  });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1 for POST");
  t.equal(stats.responsesReceived, 1, "responsesReceived should be 1 for POST");
});

test("Agent stats() tracks different HTTP methods", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/get"), { method: "GET", agent });
  await faithFetch(url("/post"), {
    method: "POST",
    body: "test",
    agent,
  });
  await faithFetch(url("/put"), {
    method: "PUT",
    body: "test",
    agent,
  });
  await faithFetch(url("/delete"), { method: "DELETE", agent });

  const stats = agent.stats();
  t.equal(
    stats.requestsSent,
    4,
    "requestsSent should be 4 for different methods",
  );
  t.equal(
    stats.responsesReceived,
    4,
    "responsesReceived should be 4 for different methods",
  );
});

test("Agent stats() tracks error responses", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/status/404"), { agent });
  await faithFetch(url("/status/500"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 2, "requestsSent should be 2 even with errors");
  t.equal(
    stats.responsesReceived,
    2,
    "responsesReceived should be 2 even with errors",
  );
});

test("Agent stats() with cookies enabled", async (t) => {
  t.plan(2);

  const agent = new Agent({ cookies: true });

  agent.addCookie(url("/cookies"), "session=test");
  await faithFetch(url("/cookies"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(stats.responsesReceived, 1, "responsesReceived should be 1");
});

test("Agent stats() with custom headers", async (t) => {
  t.plan(2);

  const agent = new Agent({
    headers: [{ name: "X-Custom", value: "test" }],
  });

  await faithFetch(url("/headers"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(stats.responsesReceived, 1, "responsesReceived should be 1");
});

test("Agent stats() with custom userAgent", async (t) => {
  t.plan(2);

  const agent = new Agent({ userAgent: "CustomAgent/1.0" });

  await faithFetch(url("/headers"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(stats.responsesReceived, 1, "responsesReceived should be 1");
});

test("Agent stats() with all options combined", async (t) => {
  t.plan(2);

  const agent = new Agent({
    cookies: true,
    userAgent: "TestAgent/1.0",
    headers: [{ name: "X-Test", value: "value" }],
  });

  await faithFetch(url("/headers"), { agent });
  await faithFetch(url("/get"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 2, "requestsSent should be 2");
  t.equal(stats.responsesReceived, 2, "responsesReceived should be 2");
});

test("Agent stats() can be called multiple times", async (t) => {
  t.plan(4);

  const agent = new Agent();

  await faithFetch(url("/get"), { agent });

  const stats1 = agent.stats();
  const stats2 = agent.stats();

  t.equal(stats1.requestsSent, 1, "First call: requestsSent should be 1");
  t.equal(stats2.requestsSent, 1, "Second call: requestsSent should be 1");
  t.equal(
    stats1.responsesReceived,
    1,
    "First call: responsesReceived should be 1",
  );
  t.equal(
    stats2.responsesReceived,
    1,
    "Second call: responsesReceived should be 1",
  );
});

test("Agent stats() returns independent objects", async (t) => {
  t.plan(3);

  const agent = new Agent();

  await faithFetch(url("/get"), { agent });

  const stats1 = agent.stats();
  const stats2 = agent.stats();

  t.notEqual(stats1, stats2, "Should return different object instances");
  t.equal(stats1.requestsSent, stats2.requestsSent, "But values should match");
  t.equal(
    stats1.responsesReceived,
    stats2.responsesReceived,
    "Values should match",
  );
});

test("Agent stats() with timeout", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/delay/1"), { agent, timeout: 5000 });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(stats.responsesReceived, 1, "responsesReceived should be 1");
});

test("Agent stats() with redirects", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/redirect/2"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should count original request");
  t.equal(
    stats.responsesReceived,
    1,
    "responsesReceived should count final response",
  );
});

test("Agent stats() tracks requests to different domains", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/get"), { agent });
  await faithFetch(url("/headers"), { agent });
  await faithFetch(url("/status/200"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 3, "Should track all requests");
  t.equal(stats.responsesReceived, 3, "Should track all responses");
});

test("Agent stats() with large number of requests", async (t) => {
  t.plan(2);

  const agent = new Agent();
  const count = 10;

  const promises = [];
  for (let i = 0; i < count; i++) {
    promises.push(faithFetch(url("/get"), { agent }));
  }

  await Promise.all(promises);

  const stats = agent.stats();
  t.equal(stats.requestsSent, count, `requestsSent should be ${count}`);
  t.equal(
    stats.responsesReceived,
    count,
    `responsesReceived should be ${count}`,
  );
});

test("Agent stats() properties are numbers", async (t) => {
  t.plan(2);

  const agent = new Agent();
  await faithFetch(url("/get"), { agent });

  const stats = agent.stats();

  t.equal(
    typeof stats.requestsSent,
    "number",
    "requestsSent should be a number",
  );
  t.equal(
    typeof stats.responsesReceived,
    "number",
    "responsesReceived should be a number",
  );
});

test("Agent stats() with mixed success and error responses", async (t) => {
  t.plan(2);

  const agent = new Agent();

  await faithFetch(url("/status/200"), { agent });
  await faithFetch(url("/status/404"), { agent });
  await faithFetch(url("/get"), { agent });
  await faithFetch(url("/status/500"), { agent });

  const stats = agent.stats();
  t.equal(stats.requestsSent, 4, "requestsSent should count all requests");
  t.equal(
    stats.responsesReceived,
    4,
    "responsesReceived should count all responses",
  );
});

test("Agent stats() before any requests", (t) => {
  t.plan(2);

  const agent = new Agent({ cookies: true, userAgent: "Test/1.0" });
  const stats = agent.stats();

  t.equal(
    stats.requestsSent,
    0,
    "requestsSent should be 0 with options but no requests",
  );
  t.equal(
    stats.responsesReceived,
    0,
    "responsesReceived should be 0 with options but no requests",
  );
});

test("Agent stats() with body streaming", async (t) => {
  t.plan(2);

  const agent = new Agent();

  const response = await faithFetch(url("/stream/20"), { agent });
  await response.text();

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(stats.responsesReceived, 1, "responsesReceived should be 1");
});

test("Agent stats() sequential vs parallel have same result", async (t) => {
  t.plan(4);

  const agent1 = new Agent();
  const agent2 = new Agent();

  await faithFetch(url("/get"), { agent: agent1 });
  await faithFetch(url("/headers"), { agent: agent1 });
  await faithFetch(url("/status/200"), { agent: agent1 });

  await Promise.all([
    faithFetch(url("/get"), { agent: agent2 }),
    faithFetch(url("/headers"), { agent: agent2 }),
    faithFetch(url("/status/200"), { agent: agent2 }),
  ]);

  const stats1 = agent1.stats();
  const stats2 = agent2.stats();

  t.equal(stats1.requestsSent, 3, "Sequential: requestsSent should be 3");
  t.equal(stats2.requestsSent, 3, "Parallel: requestsSent should be 3");
  t.equal(
    stats1.responsesReceived,
    3,
    "Sequential: responsesReceived should be 3",
  );
  t.equal(
    stats2.responsesReceived,
    3,
    "Parallel: responsesReceived should be 3",
  );
});

test("Agent stats() with network error (responsesReceived < requestsSent)", async (t) => {
  t.plan(3);

  const agent = new Agent();

  try {
    await faithFetch("http://localhost:1", { agent, timeout: 100 });
  } catch (err) {
    t.ok(err, "Should throw error for connection failure");
  }

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(
    stats.responsesReceived,
    0,
    "responsesReceived should be 0 for network error",
  );
});

test("Agent stats() with timeout (responsesReceived < requestsSent)", async (t) => {
  t.plan(3);

  const agent = new Agent();

  try {
    await faithFetch(url("/delay/10"), { agent, timeout: 100 });
  } catch (err) {
    t.ok(err, "Should throw error for timeout");
  }

  const stats = agent.stats();
  t.equal(stats.requestsSent, 1, "requestsSent should be 1");
  t.equal(
    stats.responsesReceived,
    0,
    "responsesReceived should be 0 for timeout",
  );
});

test("Agent stats() with multiple failures", async (t) => {
  t.plan(2);

  const agent = new Agent();

  try {
    await faithFetch("http://localhost:1", { agent, timeout: 100 });
  } catch (err) {}

  try {
    await faithFetch("http://localhost:2", { agent, timeout: 100 });
  } catch (err) {}

  try {
    await faithFetch(url("/delay/10"), { agent, timeout: 100 });
  } catch (err) {}

  const stats = agent.stats();
  t.equal(stats.requestsSent, 3, "requestsSent should be 3");
  t.equal(
    stats.responsesReceived,
    0,
    "responsesReceived should be 0 for all failures",
  );
});

test("Agent stats() with mixed success and failure", async (t) => {
  t.plan(3);

  const agent = new Agent();

  await faithFetch(url("/get"), { agent });

  try {
    await faithFetch("http://localhost:1", { agent, timeout: 100 });
  } catch (err) {}

  await faithFetch(url("/headers"), { agent });

  try {
    await faithFetch(url("/delay/10"), { agent, timeout: 100 });
  } catch (err) {}

  const stats = agent.stats();
  t.equal(stats.requestsSent, 4, "requestsSent should be 4");
  t.equal(
    stats.responsesReceived,
    2,
    "responsesReceived should be 2 (only successful requests)",
  );
  t.ok(
    stats.responsesReceived < stats.requestsSent,
    "responsesReceived should be less than requestsSent",
  );
});

test("Agent stats() with invalid URL", async (t) => {
  t.plan(3);

  const agent = new Agent();

  try {
    await faithFetch("not-a-valid-url", { agent });
  } catch (err) {
    t.ok(err, "Should throw error for invalid URL");
  }

  const stats = agent.stats();
  t.equal(
    stats.requestsSent,
    0,
    "requestsSent should be 0 (validated before request)",
  );
  t.equal(
    stats.responsesReceived,
    0,
    "responsesReceived should be 0 for invalid URL",
  );
});

test("Agent stats() parallel requests with some failures", async (t) => {
  t.plan(3);

  const agent = new Agent();

  const promises = [
    faithFetch(url("/get"), { agent }),
    faithFetch("http://localhost:1", { agent, timeout: 100 }).catch(() => {}),
    faithFetch(url("/headers"), { agent }),
    faithFetch(url("/delay/10"), { agent, timeout: 100 }).catch(() => {}),
    faithFetch(url("/status/200"), { agent }),
  ];

  await Promise.all(promises);

  const stats = agent.stats();
  t.equal(stats.requestsSent, 5, "requestsSent should be 5");
  t.equal(
    stats.responsesReceived,
    3,
    "responsesReceived should be 3 (only successful)",
  );
  t.ok(
    stats.responsesReceived < stats.requestsSent,
    "responsesReceived should be less than requestsSent with failures",
  );
});
