// The fetch standard reserves streaming request bodies for HTTP/2 and HTTP/3, and the agent's
// quirks group opts back out of that (spec:REQ#streaming-a-request-body, spec:QUIRK).
//
// The httpbin origin is plain HTTP/1.1, so it is the HTTP/1.x side of the rule throughout.

const fs = require("node:fs");
const http = require("node:http");
const http2 = require("node:http2");
const https = require("node:https");
const test = require("tape");
const { ReadableStream } = require("stream/web");
const { fetch, Agent, ERROR_CODES } = require("../wrapper.js");
const { ensureCert } = require("./fixtures/net.js");
const { streamingAgent, url } = require("./helpers.js");

// An HTTP/1.1 origin that counts what actually arrived.
function countingOrigin() {
	const state = { requests: 0, bytes: 0 };
	const server = http.createServer((req, res) => {
		state.requests += 1;
		req.on("data", (chunk) => {
			state.bytes += chunk.length;
		});
		req.on("end", () => {
			res.writeHead(200).end("ok");
		});
	});

	return new Promise((resolve) => {
		server.listen(0, "127.0.0.1", () => {
			resolve({
				state,
				origin: `http://127.0.0.1:${server.address().port}`,
				close: () => new Promise((done) => server.close(done)),
			});
		});
	});
}

// A TLS origin that offers exactly one protocol over ALPN, counting what arrived. Over TLS the
// protocol is only known once ALPN has run, so these are the cases that exercise the check the
// transport makes on Faith's behalf rather than the scheme shortcut.
function tlsOrigin(alpn) {
	const { ca, certPath, keyPath } = ensureCert();
	const state = { requests: 0, bytes: 0 };
	const tls = {
		key: fs.readFileSync(keyPath),
		cert: fs.readFileSync(certPath),
		ALPNProtocols: [alpn],
	};

	const server =
		alpn === "h2"
			? http2.createSecureServer(tls, (req, res) => {
					state.requests += 1;
					req.on("data", (chunk) => {
						state.bytes += chunk.length;
					});
					req.on("end", () => res.writeHead(200).end("ok"));
				})
			: https.createServer(tls, (req, res) => {
					state.requests += 1;
					req.on("data", (chunk) => {
						state.bytes += chunk.length;
					});
					req.on("end", () => res.writeHead(200).end("ok"));
				});

	return new Promise((resolve) => {
		server.listen(0, "127.0.0.1", () => {
			resolve({
				state,
				ca,
				origin: `https://localhost:${server.address().port}`,
				close: () => new Promise((done) => server.close(done)),
			});
		});
	});
}

function streamOf(...parts) {
	return new ReadableStream({
		start(controller) {
			for (const part of parts) {
				controller.enqueue(new TextEncoder().encode(part));
			}
			controller.close();
		},
	});
}

test("a streaming body over HTTP/1.1 is refused by default", async (t) => {
	t.plan(3);

	try {
		await fetch(url("/post"), {
			method: "POST",
			body: streamOf("payload"),
			duplex: "half",
		});
		t.fail("should have been refused");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.Network, "carries the Network code");
		t.equal(err.name, "NetworkError", "and the NetworkError shape");
		t.ok(
			err.message.includes("quirks.h1RequestStreaming"),
			"the message names the way to allow it",
		);
	}
});

test("the refusal holds on an agent that set no quirks", async (t) => {
	t.plan(1);

	const agent = new Agent();
	try {
		await fetch(url("/post"), {
			method: "POST",
			body: streamOf("payload"),
			duplex: "half",
			agent,
		});
		t.fail("should have been refused");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.Network, "an agent with no options is compliant");
	} finally {
		agent.close();
	}
});

test("the quirk sends a streaming body over HTTP/1.1", async (t) => {
	t.plan(2);

	const agent = streamingAgent();
	try {
		const response = await fetch(url("/post"), {
			method: "POST",
			body: streamOf("streamed ", "over ", "h1"),
			duplex: "half",
			agent,
			headers: { "Content-Type": "text/plain" },
		});

		t.equal(response.status, 200, "the request goes out");

		const json = await response.json();
		t.equal(json.data, "streamed over h1", "and the origin gets the whole body");
	} finally {
		agent.close();
	}
});

test("a refused request never reaches the origin", async (t) => {
	t.plan(3);

	// The rule is about not committing an unlengthed body to a connection that cannot carry
	// it, so the refusal is only worth anything if nothing was written to the wire.
	const server = await countingOrigin();
	try {
		await fetch(`${server.origin}/`, {
			method: "POST",
			body: streamOf("payload"),
			duplex: "half",
		});
		t.fail("should have been refused");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.Network, "refused");
		t.equal(server.state.requests, 0, "the origin saw no request");
		t.equal(server.state.bytes, 0, "and none of the body");
	} finally {
		await server.close();
	}
});

test("the quirk delivers the whole streamed body to an HTTP/1.1 origin", async (t) => {
	t.plan(3);

	const server = await countingOrigin();
	const agent = streamingAgent();
	try {
		const response = await fetch(`${server.origin}/`, {
			method: "POST",
			body: streamOf("abc", "de"),
			duplex: "half",
			agent,
		});

		t.equal(response.status, 200, "the request goes out");
		t.equal(server.state.requests, 1, "the origin saw it");
		t.equal(server.state.bytes, 5, "with every byte of the body");
	} finally {
		agent.close();
		await server.close();
	}
});

test("an HTTPS origin that negotiates HTTP/1.1 refuses a streaming body", async (t) => {
	t.plan(3);

	const server = await tlsOrigin("http/1.1");
	const agent = new Agent({ tls: { extraRoots: [server.ca] } });
	try {
		await fetch(`${server.origin}/`, {
			method: "POST",
			body: streamOf("payload"),
			duplex: "half",
			agent,
		});
		t.fail("should have been refused");
	} catch (err) {
		t.equal(err.code, ERROR_CODES.Network, "refused once ALPN settled on HTTP/1.1");
		t.equal(server.state.requests, 0, "the origin saw no request");
		t.equal(server.state.bytes, 0, "and none of the body");
	} finally {
		agent.close();
		await server.close();
	}
});

test("an HTTP/2 origin takes a streaming body with no quirk set", async (t) => {
	t.plan(3);

	const server = await tlsOrigin("h2");
	const agent = new Agent({ tls: { extraRoots: [server.ca] } });
	try {
		const response = await fetch(`${server.origin}/`, {
			method: "POST",
			body: streamOf("abc", "de"),
			duplex: "half",
			agent,
		});

		t.equal(response.status, 200, "the standard path needs no opting in");
		t.equal(server.state.requests, 1, "the origin saw it");
		t.equal(server.state.bytes, 5, "with every byte of the body");
	} finally {
		agent.close();
		await server.close();
	}
});

test("the quirk leaves an HTTP/2 origin working as it did", async (t) => {
	t.plan(2);

	// The quirk lifts a restriction rather than changing where a body goes, so an origin that
	// was already eligible behaves the same with it on.
	const server = await tlsOrigin("h2");
	const agent = new Agent({
		tls: { extraRoots: [server.ca] },
		quirks: { h1RequestStreaming: true },
	});
	try {
		const response = await fetch(`${server.origin}/`, {
			method: "POST",
			body: streamOf("abcde"),
			duplex: "half",
			agent,
		});

		t.equal(response.status, 200, "still sends");
		t.equal(server.state.bytes, 5, "with the whole body");
	} finally {
		agent.close();
		await server.close();
	}
});

test("a buffered body over HTTP/1.1 is unaffected", async (t) => {
	t.plan(2);

	// Every non-stream body type has a known length, so none of them are subject to the rule.
	const response = await fetch(url("/post"), {
		method: "POST",
		body: "buffered payload",
		headers: { "Content-Type": "text/plain" },
	});

	t.equal(response.status, 200, "a string body still posts over HTTP/1.1");

	const json = await response.json();
	t.equal(json.data, "buffered payload", "with the body intact");
});

test("a Request body is buffered during conversion, so it is unaffected", async (t) => {
	t.plan(2);

	// Converting a Request reads its body to completion, which gives it a length and takes it
	// out of the rule's scope (spec:REQ#resource).
	const request = new Request(url("/post"), {
		method: "POST",
		headers: { "Content-Type": "text/plain" },
		body: streamOf("via Request"),
		duplex: "half",
	});

	const response = await fetch(request);
	t.equal(response.status, 200, "the request goes out over HTTP/1.1");

	const json = await response.json();
	t.equal(json.data, "via Request", "with the body intact");
});

test("the quirk is scoped to the agent it is set on", async (t) => {
	t.plan(2);

	const allowed = streamingAgent();
	const refused = new Agent();
	try {
		const response = await fetch(url("/post"), {
			method: "POST",
			body: streamOf("allowed"),
			duplex: "half",
			agent: allowed,
		});
		t.equal(response.status, 200, "the agent with the quirk sends");

		try {
			await fetch(url("/post"), {
				method: "POST",
				body: streamOf("refused"),
				duplex: "half",
				agent: refused,
			});
			t.fail("the other agent should still refuse");
		} catch (err) {
			t.equal(err.code, ERROR_CODES.Network, "the agent without it still refuses");
		}
	} finally {
		allowed.close();
		refused.close();
	}
});
