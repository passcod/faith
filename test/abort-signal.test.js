const { url } = require("./helpers.js");
const test = require("tape");
const { fetch } = require("../wrapper.js");

test("signal: abort before request starts", async (t) => {
  t.plan(2);

  try {
    const controller = new AbortController();
    controller.abort();

    await fetch(url("/get"), {
      signal: controller.signal,
    });

    t.fail("Should have thrown AbortError");
  } catch (error) {
    t.equal(error.name, "AbortError", "should throw AbortError");
    t.ok(
      error.message.includes("aborted"),
      "error message should mention abort",
    );
  }
});

test("signal: abort during slow request", async (t) => {
  t.plan(1);

  try {
    const controller = new AbortController();

    setTimeout(() => {
      controller.abort();
    }, 200);

    await fetch(url("/delay/2"), {
      signal: controller.signal,
    });

    t.fail("Should have been aborted");
  } catch (error) {
    t.equal(error.name, "AbortError", "should abort the request");
  }
});

test("signal: request completes before abort", async (t) => {
  t.plan(1);

  try {
    const controller = new AbortController();

    setTimeout(() => {
      controller.abort();
    }, 3000);

    const response = await fetch(url("/get"), {
      signal: controller.signal,
    });

    t.equal(response.status, 200, "request should complete successfully");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("signal: request without signal works normally", async (t) => {
  t.plan(1);

  try {
    const response = await fetch(url("/get"));
    t.equal(response.status, 200, "should work without signal");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});

test("signal: POST request with abort", async (t) => {
  t.plan(1);

  try {
    const controller = new AbortController();

    setTimeout(() => {
      controller.abort();
    }, 200);

    await fetch(url("/delay/2"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ test: "data" }),
      signal: controller.signal,
    });

    t.fail("Should have been aborted");
  } catch (error) {
    t.equal(error.name, "AbortError", "should abort POST request");
  }
});

test("signal: multiple requests with same controller", async (t) => {
  t.plan(2);

  const controller = new AbortController();

  try {
    const req1 = fetch(url("/delay/1"), { signal: controller.signal });
    const req2 = fetch(url("/delay/2"), { signal: controller.signal });

    setTimeout(() => {
      controller.abort();
    }, 100);

    await Promise.all([req1, req2]);
    t.fail("Both requests should have been aborted");
  } catch (error) {
    t.equal(error.name, "AbortError", "should abort first request");
  }

  try {
    await fetch(url("/get"), { signal: controller.signal });
    t.fail("Should not allow new request with aborted signal");
  } catch (error) {
    t.equal(error.name, "AbortError", "should reject with aborted signal");
  }
});

test("signal: works with Request object", async (t) => {
  t.plan(1);

  try {
    const controller = new AbortController();
    const request = new Request(url("/delay/2"));

    setTimeout(() => {
      controller.abort();
    }, 200);

    await fetch(request, {
      signal: controller.signal,
    });

    t.fail("Should have been aborted");
  } catch (error) {
    t.equal(error.name, "AbortError", "should abort Request object fetch");
  }
});

test("signal: abort with timeout option", async (t) => {
  t.plan(1);

  try {
    const controller = new AbortController();

    setTimeout(() => {
      controller.abort();
    }, 300);

    await fetch(url("/delay/2"), {
      signal: controller.signal,
      timeout: 5000,
    });

    t.fail("Should have been aborted");
  } catch (error) {
    t.equal(
      error.name,
      "AbortError",
      "abort should take precedence over timeout",
    );
  }
});

test("signal: abort does not prevent reading completed response", async (t) => {
  t.plan(2);

  try {
    const controller = new AbortController();
    const response = await fetch(url("/get"), {
      signal: controller.signal,
    });

    t.equal(response.status, 200, "request should complete");

    controller.abort();

    const data = await response.json();
    t.ok(data, "should be able to read response after aborting controller");
  } catch (error) {
    t.fail(`Unexpected error: ${error.message}`);
  }
});
