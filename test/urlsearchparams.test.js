const test = require("tape");
const { fetch } = require("../wrapper.js");

const HTTPBIN_URL = process.env.HTTPBIN_URL;

test("POST with URLSearchParams body", async (t) => {
	const params = new URLSearchParams();
	params.append("foo", "bar");
	params.append("baz", "qux");
	params.append("special", "hello world");

	const response = await fetch(`${HTTPBIN_URL}/post`, {
		method: "POST",
		body: params,
	});

	t.equal(response.status, 200, "status should be 200");

	const json = await response.json();
	t.ok(
		json.headers["Content-Type"].includes(
			"application/x-www-form-urlencoded;charset=UTF-8",
		),
		"Content-Type header should be set automatically",
	);
	t.equal(json.form.foo[0], "bar", "form data should contain foo=bar");
	t.equal(json.form.baz[0], "qux", "form data should contain baz=qux");
	t.equal(
		json.form.special[0],
		"hello world",
		"form data should contain special=hello world",
	);
});

test("POST with URLSearchParams body and custom Content-Type", async (t) => {
	const params = new URLSearchParams();
	params.append("key", "value");

	const response = await fetch(`${HTTPBIN_URL}/post`, {
		method: "POST",
		headers: {
			"Content-Type": "application/x-www-form-urlencoded",
		},
		body: params,
	});

	t.equal(response.status, 200, "status should be 200");

	const json = await response.json();
	t.ok(
		json.headers["Content-Type"].includes(
			"application/x-www-form-urlencoded",
		),
		"Custom Content-Type header should be preserved",
	);
	t.equal(json.form.key[0], "value", "form data should contain key=value");
});

test("POST with empty URLSearchParams body", async (t) => {
	const params = new URLSearchParams();

	const response = await fetch(`${HTTPBIN_URL}/post`, {
		method: "POST",
		body: params,
	});

	t.equal(response.status, 200, "status should be 200");

	const json = await response.json();
	t.ok(
		json.headers["Content-Type"].includes(
			"application/x-www-form-urlencoded;charset=UTF-8",
		),
		"Content-Type header should be set automatically",
	);
	t.deepEqual(json.form, {}, "form data should be empty");
});

test("PUT with URLSearchParams body", async (t) => {
	const params = new URLSearchParams();
	params.append("action", "update");

	const response = await fetch(`${HTTPBIN_URL}/put`, {
		method: "PUT",
		body: params,
	});

	t.equal(response.status, 200, "status should be 200");

	const json = await response.json();
	t.equal(
		json.form.action[0],
		"update",
		"form data should contain action=update",
	);
});
