const { url } = require("./helpers.js");
const test = require("tape");
const { fetch } = require("../wrapper.js");

// Helper to get cookies from response (go-httpbin returns cookies at root level)
function getCookies(data) {
	// If data has typical httpbin fields, cookies are not at root level
	if (data.headers || data.url || data.method) {
		return {};
	}
	// Otherwise, the entire response is cookies (for /cookies endpoint)
	return data;
}

test("credentials: default behavior is include", async (t) => {
	t.plan(1);

	try {
		// With credentials in URL, default should allow them
		const response = await fetch(
			"http://user:pass@localhost:8888/basic-auth/user/pass",
		);
		t.equal(
			response.status,
			200,
			"should authenticate with default settings",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: explicit 'include' allows URL credentials", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(
			"http://user:pass@localhost:8888/basic-auth/user/pass",
			{
				credentials: "include",
			},
		);
		t.equal(
			response.status,
			200,
			"should authenticate with credentials: include",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'same-origin' is transformed to 'include'", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(
			"http://user:pass@localhost:8888/basic-auth/user/pass",
			{
				credentials: "same-origin",
			},
		);
		t.equal(
			response.status,
			200,
			"should authenticate with credentials: same-origin (transformed to include)",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'omit' strips credentials from URL", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(
			"http://user:pass@localhost:8888/basic-auth/user/pass",
			{
				credentials: "omit",
			},
		);
		t.equal(
			response.status,
			401,
			"should fail authentication when credentials are omitted",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'omit' filters Cookie header from request", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(url("/cookies"), {
			credentials: "omit",
			headers: {
				Cookie: "test=value; session=abc123; token=xyz",
			},
		});

		const data = await response.json();
		const cookies = getCookies(data);
		t.equal(
			Object.keys(cookies).length,
			0,
			"should not send any cookies when credentials is omit",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'include' preserves Cookie header in request", async (t) => {
	t.plan(2);

	try {
		const response = await fetch(url("/cookies"), {
			credentials: "include",
			headers: {
				Cookie: "test=value; another=data",
			},
		});

		const data = await response.json();
		const cookies = getCookies(data);
		t.equal(cookies.test, "value", "should send first cookie");
		t.equal(cookies.another, "data", "should send second cookie");
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: default (include) preserves Cookie header", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(url("/cookies"), {
			headers: {
				Cookie: "defaulttest=defaultvalue",
			},
		});

		const data = await response.json();
		const cookies = getCookies(data);
		t.equal(
			cookies.defaulttest,
			"defaultvalue",
			"should send cookie with default credentials",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'omit' filters Set-Cookie from response", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(
			url("/response-headers?Set-Cookie=testcookie=testvalue"),
			{
				credentials: "omit",
			},
		);

		const setCookie = response.headers.get("set-cookie");
		t.equal(
			setCookie,
			null,
			"should not include Set-Cookie header when credentials is omit",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'include' preserves Set-Cookie in response", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(
			url("/response-headers?Set-Cookie=testcookie=testvalue"),
			{
				credentials: "include",
			},
		);

		const setCookie = response.headers.get("set-cookie");
		t.equal(
			setCookie,
			"testcookie=testvalue",
			"should include Set-Cookie header when credentials is include",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: default (include) preserves Set-Cookie in response", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(
			url("/response-headers?Set-Cookie=defaultcookie=defaultvalue"),
		);

		const setCookie = response.headers.get("set-cookie");
		t.equal(
			setCookie,
			"defaultcookie=defaultvalue",
			"should include Set-Cookie header with default credentials",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'omit' filters multiple Set-Cookie headers", async (t) => {
	t.plan(2);

	try {
		const response = await fetch(
			url(
				"/response-headers?Set-Cookie=cookie1=value1&Set-Cookie=cookie2=value2",
			),
			{
				credentials: "omit",
			},
		);

		const setCookie = response.headers.get("set-cookie");
		t.equal(setCookie, null, "should not include any Set-Cookie headers");

		let cookieCount = 0;
		response.headers.forEach((value, name) => {
			if (name.toLowerCase() === "set-cookie") {
				cookieCount++;
			}
		});
		t.equal(cookieCount, 0, "should have zero Set-Cookie headers");
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'omit' with POST request", async (t) => {
	t.plan(2);

	try {
		const response = await fetch(url("/post"), {
			method: "POST",
			credentials: "omit",
			headers: {
				"Content-Type": "application/json",
				Cookie: "shouldnotbesent=value",
			},
			body: JSON.stringify({ test: "data" }),
		});

		const data = await response.json();
		t.equal(data.json.test, "data", "should send POST body correctly");
		const cookies = getCookies(data);
		t.equal(Object.keys(cookies).length, 0, "should not send cookies");
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: 'omit' is case-insensitive for Cookie header", async (t) => {
	t.plan(1);

	try {
		const response = await fetch(url("/cookies"), {
			credentials: "omit",
			headers: {
				COOKIE: "uppercase=test",
				cookie: "lowercase=test",
				Cookie: "mixedcase=test",
			},
		});

		const data = await response.json();
		const cookies = getCookies(data);
		t.equal(
			Object.keys(cookies).length,
			0,
			"should filter Cookie header regardless of case",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: works with Request object", async (t) => {
	t.plan(1);

	try {
		const request = new Request(url("/cookies"), {
			credentials: "omit",
			headers: {
				Cookie: "test=value",
			},
		});

		const response = await fetch(request);
		const data = await response.json();
		const cookies = getCookies(data);
		t.equal(
			Object.keys(cookies).length,
			0,
			"should respect credentials from Request object",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: options override Request object", async (t) => {
	t.plan(1);

	try {
		const request = new Request(url("/cookies"), {
			credentials: "omit",
			headers: {
				Cookie: "test=value",
			},
		});

		const response = await fetch(request, {
			credentials: "include",
		});

		const data = await response.json();
		const cookies = getCookies(data);
		t.equal(
			cookies.test,
			"value",
			"should use credentials from options, overriding Request",
		);
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});

test("credentials: URL object with credentials", async (t) => {
	t.plan(2);

	try {
		const urlObj = new URL("http://localhost:8888/basic-auth/user/pass");
		urlObj.username = "user";
		urlObj.password = "pass";

		const response1 = await fetch(urlObj, { credentials: "include" });
		t.equal(response1.status, 200, "should authenticate with include");

		const response2 = await fetch(urlObj, { credentials: "omit" });
		t.equal(response2.status, 401, "should fail with omit");
	} catch (error) {
		t.fail(`Unexpected error: ${error.message}`);
	}
});
