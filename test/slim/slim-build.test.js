// A binding built with capabilities left out: it must load, serve an ordinary request, and refuse
// what it cannot do rather than ignoring the ask.
//
// Not part of the default suite: it needs a `.node` built with `--no-default-features`, which is
// not the one the rest of the suite runs against. See `npm run test:slim`.

const test = require("tape");
const { Agent, fetch } = require("../../wrapper.js");

const HTTPBIN_URL = process.env.HTTPBIN_URL || "http://localhost:8888";

test("a slim binding loads and serves an ordinary request", async (t) => {
	const agent = new Agent();
	const response = await fetch(`${HTTPBIN_URL}/get`, { agent });
	t.equal(response.status, 200, "a plain GET still works with everything optional left out");
	const body = await response.json();
	t.ok(body.url.endsWith("/get"), "and the body reads");
	agent.close();
});

test("a slim binding drops the methods whose capability is gone", (t) => {
	const agent = new Agent();
	t.equal(typeof agent.addCookie, "undefined", "no addCookie without the cookies feature");
	t.equal(typeof agent.getCookie, "undefined", "no getCookie without the cookies feature");
	t.equal(typeof agent.resolvers, "undefined", "no resolvers() without the dns feature");
	t.equal(
		typeof agent.connections,
		"undefined",
		"no connections() without connection tracking",
	);
	agent.close();
	t.end();
});

test("a slim binding refuses an agent option it cannot honour", (t) => {
	for (const [option, group] of [
		[{ cookies: true }, "cookie"],
		[{ cache: { store: "memory" } }, "HTTP cache"],
		[{ dns: { system: true } }, "resolver"],
		[{ http3: { upgradeEnabled: true } }, "HTTP/3"],
	]) {
		t.throws(
			() => new Agent(option),
			new RegExp(group),
			`${JSON.stringify(option)} is refused, naming ${group}`,
		);
	}
	t.end();
});

test("a slim binding refuses a per-request option it cannot honour", async (t) => {
	const agent = new Agent();

	for (const [options, expected] of [
		[{ compress: "gzip" }, /content-coding/],
		[{ cache: "no-store" }, /HTTP cache/],
	]) {
		try {
			await fetch(`${HTTPBIN_URL}/get`, { agent, ...options });
			t.fail(`${JSON.stringify(options)} should be refused`);
		} catch (err) {
			t.match(
				err.message,
				expected,
				`${JSON.stringify(options)} is refused, saying why`,
			);
		}
	}

	agent.close();
});

test("dns.overrides still works, reaching reqwest rather than Faith's resolver", async (t) => {
	// The one DNS setting that is not the resolver's, so a slim build honours it.
	const agent = new Agent({
		dns: { overrides: [{ domain: "faith.invalid", addresses: [] }] },
	});
	t.ok(agent, "an overrides-only dns group is accepted");
	agent.close();
	t.end();
});
