const test = require("tape");
const net = require("node:net");
const { compareResponses, hasNativeFetch, url } = require("./helpers.js");
const { fetch, Agent, ERROR_CODES } = require("../wrapper.js");
const native = require("../index.js");

/** The methods the fetch standard normalises to upper case. */
const NORMALISED = ["delete", "get", "head", "options", "post", "put"];

/**
 * An origin that echoes the request method back in `x-method`.
 *
 * A raw TCP listener rather than `node:http`: llhttp parses methods against its own
 * table, so a custom method like `Frobnicate` is rejected before any handler sees it,
 * which is precisely the case under test. The method goes in a header rather than the
 * body so a HEAD request can be asserted the same way as the rest.
 */
async function methodEcho() {
	const sockets = new Set();
	const server = net.createServer((socket) => {
		sockets.add(socket);
		socket.on("close", () => sockets.delete(socket));

		let request = "";
		let answered = false;
		socket.on("data", (chunk) => {
			if (answered) return;
			request += chunk;
			if (!request.includes("\r\n\r\n")) return;
			answered = true;
			const method = request.slice(0, request.indexOf(" "));
			socket.end(
				`HTTP/1.1 200 OK\r\nx-method: ${method}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n`,
			);
		});
	});

	await new Promise((resolve, reject) => {
		server.once("error", reject);
		server.listen(0, "127.0.0.1", resolve);
	});

	return {
		url: `http://127.0.0.1:${server.address().port}/`,
		close: () =>
			new Promise((resolve) => {
				for (const socket of sockets) socket.destroy();
				server.close(resolve);
			}),
	};
}

test("Compare different HTTP methods", { skip: !hasNativeFetch }, async (t) => {
  const methods = ["GET", "POST", "PUT", "DELETE", "PATCH"];

  for (const method of methods) {
    const path = `/${method.toLowerCase()}`;
    const options = { method };

    if (method === "POST" || method === "PUT" || method === "PATCH") {
      options.body = Array.from(Buffer.from("test body"));
    }

    await compareResponses(t, path, options);
  }
});

test("the methods the standard normalises are sent upper case", async (t) => {
  const origin = await methodEcho();

  try {
    for (const method of [...NORMALISED, "GeT", "Put", "hEaD"]) {
      const response = await fetch(origin.url, { method });
      t.equal(
        response.headers.get("x-method"),
        method.toUpperCase(),
        `${method} should reach the server as ${method.toUpperCase()}`,
      );
    }
  } finally {
    await origin.close();
  }
});

test("every other method is sent with the case it was given", async (t) => {
  const origin = await methodEcho();

  try {
    for (const method of ["patch", "Frobnicate", "m-SEARCH", "purge"]) {
      const response = await fetch(origin.url, { method });
      t.equal(
        response.headers.get("x-method"),
        method,
        `${method} should reach the server unchanged`,
      );
    }
  } finally {
    await origin.close();
  }
});

test("a custom method keeps its case across a redirect that preserves it", async (t) => {
  const response = await fetch(
    url("/redirect-to?url=/anything&status_code=307"),
    { method: "Frobnicate" },
  );
  const body = await response.json();

  t.equal(body.method, "Frobnicate", "the replayed method should be unchanged");
});

test("a custom method goes through an agent with a cache", async (t) => {
  const agent = new Agent({ cache: { store: "memory" } });
  const response = await fetch(url("/anything"), {
    method: "Frobnicate",
    agent,
  });
  const body = await response.json();

  t.equal(body.method, "Frobnicate", "the method should be unchanged");
});

test("a normalised method given in lower case still behaves as itself", async (t) => {
  const response = await fetch(url("/get"), { method: "head" });

  t.equal(response.status, 200, "should reach /get as a HEAD request");
  t.equal(await response.text(), "", "a HEAD response should carry no body");
});

test(
  "a method that only matches under Unicode case folding is invalid",
  { skip: !hasNativeFetch },
  async (t) => {
    t.plan(1);

    // Uppercasing "OPTıONS" the Unicode way yields "OPTIONS", which would both
    // normalise it and smuggle a method with non-token bytes past validation.
    try {
      await fetch(url("/get"), { method: "OPTıONS" });
      t.fail("Should have thrown error when using a non-ASCII method");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.InvalidMethod,
        "should set canonical error code 'InvalidMethod'",
      );
    }
  },
);

test(
  "fetch rejects invalid HTTP method",
  { skip: !hasNativeFetch },
  async (t) => {
    t.plan(1);

    try {
      await fetch(url("/get"), { method: "INV@LID-METHOD!" });
      t.fail("Should have thrown error when using invalid HTTP method");
    } catch (error) {
      t.equal(
        error.code,
        ERROR_CODES.InvalidMethod,
        "should set canonical error code 'InvalidMethod'",
      );
    }
  },
);
