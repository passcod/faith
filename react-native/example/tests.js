import { ERROR_CODES, fetch } from "@passcod/faith-react-native";

// Hand-rolled rather than `node:assert`, which Metro has no module to resolve.

function ok(value, message) {
	if (!value) {
		throw new Error(message);
	}
}

function equal(actual, expected, message) {
	ok(
		actual === expected,
		message ?? `expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
	);
}

async function rejects(promise, matches, message) {
	try {
		await promise;
	} catch (error) {
		ok(matches(error), message ?? `rejected with the wrong error: ${error}`);
		return;
	}
	throw new Error(message ?? "expected a rejection, but it resolved");
}

/**
 * The scenarios the example exists to prove.
 *
 * Plain HTTP goes to the httpbin the repository's own suite uses, reached over an `adb
 * reverse` tunnel so the emulator sees it on loopback and the results stay deterministic.
 *
 * One case reaches a public HTTPS host, because nothing local can exercise the device's
 * own trust store: that is the point of it. Its address is a parameter so a run with no
 * outbound network can point it elsewhere.
 *
 * @param {object} context
 * @param {string} [context.httpbinUrl]
 * @param {string} [context.httpsUrl]
 */
export function loadTests({
	httpbinUrl = "http://127.0.0.1:8888",
	httpsUrl = "https://example.com/",
}) {
	describe("Faith under React Native", () => {
		it("resolves a GET and reads the body", async () => {
			// The whole card in one line: a Tokio runtime on a background thread settling a
			// promise on Hermes' single JavaScript thread.
			const response = await fetch(`${httpbinUrl}/get`);
			equal(response.status, 200);
			const body = JSON.parse(await response.text());
			equal(typeof body.url, "string");
		});

		it("reads JSON and headers", async () => {
			const response = await fetch(`${httpbinUrl}/json`);
			equal(response.headers.get("content-type"), "application/json; charset=utf-8");
			equal(typeof (await response.json()), "object");
		});

		it("sends a POST body", async () => {
			const response = await fetch(`${httpbinUrl}/post`, {
				method: "POST",
				headers: { "content-type": "text/plain" },
				body: "faith",
			});
			equal(response.status, 200);
			equal(JSON.parse(await response.text()).data, "faith");
		});

		it("settles concurrent requests", async function () {
			// Serialisation here would still pass every test above, so the count is what
			// makes it meaningful: ten threadsafe-function callbacks against one JS thread.
			this.timeout(30_000);
			const responses = await Promise.all(
				Array.from({ length: 10 }, (_, index) =>
					fetch(`${httpbinUrl}/status/200?n=${index}`),
				),
			);
			equal(responses.filter((response) => response.status === 200).length, 10);
		});

		it("rejects an aborted request rather than hanging", async () => {
			const controller = new AbortController();
			const pending = fetch(`${httpbinUrl}/delay/10`, { signal: controller.signal });
			controller.abort();
			await rejects(pending, (error) => error.name === "AbortError");
		});

		it("rejects an unreachable host with a code", async () => {
			await rejects(
				// Reserved by RFC 6761 to never resolve.
				fetch("http://unreachable.invalid/"),
				(error) => typeof error.code === "string" && error.code in ERROR_CODES,
			);
		});

		it("verifies a certificate against the device's trust store", async function () {
			// Reaches the public internet, unlike everything above, because the Android
			// trust manager is what is under test and only a real chain exercises it. A
			// failure here most likely means the JNI handover never happened.
			this.timeout(30_000);
			const response = await fetch(httpsUrl);
			ok(response.status < 500);
		});
	});
}
