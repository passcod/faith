const test = require("tape");
const net = require("node:net");
const { url } = require("./helpers.js");
const { fetch, Agent } = require("../wrapper.js");

/** An origin that echoes every `Content-Type` it received back, one per line. */
async function typeEcho() {
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
			const head = request.slice(0, request.indexOf("\r\n\r\n"));
			const types = head
				.split("\r\n")
				.filter((line) => /^content-type:/i.test(line))
				.map((line) => line.slice(line.indexOf(":") + 1).trim());
			socket.end(
				`HTTP/1.1 200 OK\r\nx-types: ${JSON.stringify(types)}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n`,
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

/** The `Content-Type` headers the origin saw for a request carrying `body`. */
async function typesFor(origin, options) {
	const response = await fetch(origin.url, { method: "POST", ...options });
	return JSON.parse(response.headers.get("x-types"));
}

test("a body's kind implies the Content-Type the fetch standard extracts", async (t) => {
	const origin = await typeEcho();

	try {
		const formData = new FormData();
		formData.append("a", "1");

		t.deepEqual(
			await typesFor(origin, { body: "hi" }),
			["text/plain;charset=UTF-8"],
			"a string implies text/plain",
		);
		t.deepEqual(
			await typesFor(origin, { body: new URLSearchParams({ a: "1" }) }),
			["application/x-www-form-urlencoded;charset=UTF-8"],
			"a URLSearchParams implies form encoding",
		);
		t.deepEqual(
			await typesFor(origin, {
				body: new Blob(["x"], { type: "application/json" }),
			}),
			["application/json"],
			"a Blob carries its own type",
		);
		t.deepEqual(
			await typesFor(origin, {
				body: new File(["x"], "f.csv", { type: "text/csv" }),
			}),
			["text/csv"],
			"a File carries its own type",
		);

		const [multipart, ...rest] = await typesFor(origin, { body: formData });
		t.equal(rest.length, 0, "a FormData implies exactly one type");
		t.match(
			multipart,
			/^multipart\/form-data; boundary=.+/,
			"a FormData implies multipart with its boundary",
		);

		t.deepEqual(
			await typesFor(origin, { body: new Uint8Array([1, 2, 3]) }),
			[],
			"raw bytes imply nothing",
		);
		t.deepEqual(
			await typesFor(origin, { body: new Blob(["x"]) }),
			[],
			"an untyped Blob implies nothing",
		);
	} finally {
		await origin.close();
	}
});

test("a declared Content-Type outranks the one a body implies", async (t) => {
	const origin = await typeEcho();

	try {
		const agent = new Agent({
			headers: [{ name: "Content-Type", value: "application/json" }],
		});

		t.deepEqual(
			await typesFor(origin, { body: "hi", agent }),
			["application/json"],
			"an agent's default outranks the implied type",
		);
		t.deepEqual(
			await typesFor(origin, {
				body: "hi",
				headers: { "content-type": "application/xml" },
				agent,
			}),
			["application/xml"],
			"a type on the request outranks both",
		);
		t.deepEqual(
			await typesFor(origin, {
				body: new URLSearchParams({ a: "1" }),
				agent,
			}),
			["application/json"],
			"an agent's default outranks a form-encoded body's type too",
		);
	} finally {
		await origin.close();
	}
});

test("a Blob, File, or FormData body sends the bytes the standard encodes", async (t) => {
	const blob = await fetch(url("/anything"), {
		method: "POST",
		body: new Blob(["hello"], { type: "text/plain" }),
	}).then((response) => response.json());
	t.equal(blob.data, "hello", "a Blob should send its own bytes");

	const file = await fetch(url("/anything"), {
		method: "POST",
		body: new File(["in a file"], "f.txt", { type: "text/plain" }),
	}).then((response) => response.json());
	t.equal(file.data, "in a file", "a File should send its own bytes");

	const formData = new FormData();
	formData.append("colour", "orange");
	const form = await fetch(url("/anything"), {
		method: "POST",
		body: formData,
	}).then((response) => response.json());
	t.deepEqual(
		form.form?.colour,
		["orange"],
		"a FormData should send bytes the origin can parse against the boundary",
	);
});
