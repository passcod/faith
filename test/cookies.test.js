const test = require("tape");
const { fetch: faithFetch, Agent } = require("../wrapper.js");
const { url } = require("./helpers.js");

// Helper to get header value (handles both string and array)
function getHeader(headers, name) {
	const value = headers[name];
	return Array.isArray(value) ? value[0] : value;
}

// Helper to get cookies from response (go-httpbin returns cookies at root level)
function getCookies(data) {
	// If data has typical httpbin fields, cookies are not at root level
	if (data.headers || data.url || data.method) {
		return {};
	}
	// Otherwise, the entire response is cookies (for /cookies endpoint)
	return data;
}

test("Agent with cookies enabled can add and retrieve cookies", async (t) => {
	t.plan(3);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=abc123");
	const cookie = agent.getCookie(testUrl);

	t.equal(cookie, "session=abc123", "Should retrieve the added cookie");

	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should successfully fetch with cookies enabled");

	const data = await response.json();
	const cookies = getCookies(data);
	t.equal(cookies.session, "abc123", "Server should receive the cookie");
});

test("Agent without cookies enabled returns null for getCookie", (t) => {
	t.plan(1);

	const agent = new Agent({ cookies: false });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=abc123");
	const cookie = agent.getCookie(testUrl);

	t.equal(cookie, null, "Should return null when cookies are disabled");
});

test("Agent with default options (no cookies) returns null for getCookie", (t) => {
	t.plan(1);

	const agent = new Agent();
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=abc123");
	const cookie = agent.getCookie(testUrl);

	t.equal(
		cookie,
		null,
		"Should return null when cookies are not explicitly enabled",
	);
});

test("Multiple cookies can be added to an agent", async (t) => {
	t.plan(4);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=abc123");
	agent.addCookie(testUrl, "user=john");
	agent.addCookie(testUrl, "theme=dark");

	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	const cookies = getCookies(data);
	t.equal(cookies.session, "abc123", "First cookie should be sent");
	t.equal(cookies.user, "john", "Second cookie should be sent");
	t.equal(cookies.theme, "dark", "Third cookie should be sent");
});

test("Cookies persist across multiple requests", async (t) => {
	t.plan(5);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=persistent123");

	const response1 = await faithFetch(testUrl, { agent });
	t.ok(response1.ok, "First request should succeed");

	const data1 = await response1.json();
	const cookies1 = getCookies(data1);
	t.equal(
		cookies1.session,
		"persistent123",
		"First request should include cookie",
	);

	const response2 = await faithFetch(testUrl, { agent });
	t.ok(response2.ok, "Second request should succeed");

	const data2 = await response2.json();
	const cookies2 = getCookies(data2);
	t.equal(
		cookies2.session,
		"persistent123",
		"Second request should include cookie",
	);
	t.equal(
		cookies1.session,
		cookies2.session,
		"Both requests should have the same cookie",
	);
});

test("Server-set cookies are stored by agent", async (t) => {
	t.plan(4);

	const agent = new Agent({ cookies: true });
	const setCookieUrl = url("/cookies/set?name=value");

	const response1 = await faithFetch(setCookieUrl, { agent });
	t.ok(response1.ok, "Should successfully set cookie");

	const cookiesUrl = url("/cookies");
	const cookie = agent.getCookie(cookiesUrl);
	t.ok(cookie, "Agent should have stored the cookie");

	const response2 = await faithFetch(cookiesUrl, { agent });
	t.ok(response2.ok, "Should successfully fetch with stored cookie");

	const data = await response2.json();
	const cookies = getCookies(data);
	t.equal(cookies.name, "value", "Stored cookie should be sent");
});

test("Different agents have separate cookie stores", async (t) => {
	t.plan(6);

	const agent1 = new Agent({ cookies: true });
	const agent2 = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent1.addCookie(testUrl, "session=agent1");
	agent2.addCookie(testUrl, "session=agent2");

	const response1 = await faithFetch(testUrl, { agent: agent1 });
	const response2 = await faithFetch(testUrl, { agent: agent2 });

	t.ok(response1.ok, "First agent request should succeed");
	t.ok(response2.ok, "Second agent request should succeed");

	const data1 = await response1.json();
	const data2 = await response2.json();
	const cookies1 = getCookies(data1);
	const cookies2 = getCookies(data2);

	t.equal(cookies1.session, "agent1", "First agent should send its cookie");
	t.equal(cookies2.session, "agent2", "Second agent should send its cookie");
	t.notEqual(
		cookies1.session,
		cookies2.session,
		"Agents should have different cookies",
	);

	const cookie1 = agent1.getCookie(testUrl);
	t.ok(
		cookie1.includes("agent1"),
		"First agent should retrieve its own cookie",
	);
});

test("getCookie returns null for URL with no cookies", (t) => {
	t.plan(1);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/get");

	const cookie = agent.getCookie(testUrl);
	t.equal(cookie, null, "Should return null when no cookies are set");
});

test("Cookies with attributes are handled correctly", async (t) => {
	t.plan(2);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=abc123; Path=/; HttpOnly");

	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	const cookies = getCookies(data);
	t.equal(cookies.session, "abc123", "Cookie value should be sent correctly");
});

test("Adding cookie overwrites previous value", async (t) => {
	t.plan(2);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=old");
	agent.addCookie(testUrl, "session=new");

	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	const cookies = getCookies(data);
	t.equal(cookies.session, "new", "Cookie should have the updated value");
});

test("Cookies are scoped to URL domains", async (t) => {
	t.plan(3);

	const agent = new Agent({ cookies: true });
	const url1 = url("/cookies");
	const url2 = "https://example.com/cookies";

	agent.addCookie(url1, "session=local");

	const cookie1 = agent.getCookie(url1);
	const cookie2 = agent.getCookie(url2);

	t.ok(cookie1, "Should retrieve cookie for the matching URL");
	t.equal(cookie2, null, "Should not retrieve cookie for different domain");

	const response = await faithFetch(url1, { agent });
	const data = await response.json();
	const cookies = getCookies(data);
	t.equal(
		cookies.session,
		"local",
		"Cookie should only be sent to matching domain",
	);
});

test("Agent with cookies works with other options", async (t) => {
	t.plan(4);

	const agent = new Agent({ cookies: true, userAgent: "CookieAgent/1.0" });
	const cookiesUrl = url("/cookies");
	const headersUrl = url("/headers");

	agent.addCookie(cookiesUrl, "session=test");

	const response1 = await faithFetch(cookiesUrl, { agent });
	t.ok(response1.ok, "Should successfully fetch cookies endpoint");

	const data1 = await response1.json();
	const cookies1 = getCookies(data1);
	t.equal(cookies1.session, "test", "Cookie should be sent");

	const response2 = await faithFetch(headersUrl, { agent });
	const data2 = await response2.json();
	t.equal(
		getHeader(data2.headers, "User-Agent"),
		"CookieAgent/1.0",
		"Custom user agent should be used",
	);
	t.ok(response2.ok, "Should successfully fetch headers endpoint");
});

test("Parallel requests with cookies work correctly", async (t) => {
	t.plan(6);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "session=parallel");

	const promises = [
		faithFetch(testUrl, { agent }),
		faithFetch(testUrl, { agent }),
		faithFetch(testUrl, { agent }),
	];

	const responses = await Promise.all(promises);

	t.ok(responses[0].ok, "First parallel request should succeed");
	t.ok(responses[1].ok, "Second parallel request should succeed");
	t.ok(responses[2].ok, "Third parallel request should succeed");

	const data1 = await responses[0].json();
	const data2 = await responses[1].json();
	const data3 = await responses[2].json();
	const cookies1 = getCookies(data1);
	const cookies2 = getCookies(data2);
	const cookies3 = getCookies(data3);

	t.equal(cookies1.session, "parallel", "First request should have cookie");
	t.equal(cookies2.session, "parallel", "Second request should have cookie");
	t.equal(cookies3.session, "parallel", "Third request should have cookie");
});

test("Empty cookie string", (t) => {
	t.plan(1);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "");
	const cookie = agent.getCookie(testUrl);

	t.equal(cookie, null, "Empty cookie string should return null");
});

test("Complex cookie value with special characters", async (t) => {
	t.plan(2);

	const agent = new Agent({ cookies: true });
	const testUrl = url("/cookies");

	agent.addCookie(testUrl, "data=value%20with%20spaces");

	const response = await faithFetch(testUrl, { agent });
	t.ok(response.ok, "Should successfully fetch");

	const data = await response.json();
	const cookies = getCookies(data);
	t.ok(cookies.data, "Cookie with encoded value should be sent");
});
