/**
 * Faith Fetch API Wrapper
 *
 * This wrapper provides a spec-compliant Fetch API interface on top of
 * the native Rust bindings. The main difference is that `body` is exposed
 * as a property/getter instead of a method, and the class is named `Response`
 * instead of `FetchResponse`.
 */

const { fileURLToPath } = require("node:url");

const native = require("./index.js");
const { faithFetch } = native;

// Generate ERROR_CODES const enum from native error codes
// e.g. { InvalidHeader: "InvalidHeader", InvalidMethod: "InvalidMethod", ... }
const ERROR_CODES = native.errorCodes().reduce((acc, code) => {
	acc[code] = code;
	return acc;
}, {});

/**
 * A `TypeError` naming a `toFile()` destination that does not name a local path.
 * @param {string} message
 * @returns {TypeError}
 */
function invalidPathError(message) {
	const error = new TypeError(message);
	error.code = ERROR_CODES.InvalidPath;
	return error;
}

/**
 * A `TypeError` for a conversion or read refused because the body is already disturbed,
 * carrying the same `code` the native layer sets.
 * @param {unknown} [cause]
 * @returns {TypeError}
 */
function disturbedResponseError(cause) {
	const error = new TypeError("response body already disturbed", { cause });
	error.code = ERROR_CODES.ResponseAlreadyDisturbed;
	return error;
}

/**
 * Resolve a `toFile()` destination to a string path. A `file://` URL is converted here, by
 * the platform's own conversion, before the request reaches the native layer; a URL that
 * does not name a local path throws `InvalidPath`. A plain string is taken as a path.
 * @param {string | URL} destination
 * @returns {string}
 */
function destinationPath(destination) {
	if (destination instanceof URL || /^file:/i.test(destination)) {
		let parsed;
		try {
			parsed = destination instanceof URL ? destination : new URL(destination);
		} catch (cause) {
			throw invalidPathError(
				`toFile destination is not a local path: ${cause.message}`,
			);
		}

		// A host names a machine other than this one, so the URL does not name a local path.
		// Checked here rather than left to the platform: Windows' own conversion turns a host
		// into a UNC path instead of refusing it, and a network share is not a local file.
		// `localhost` normalises to an empty host, so this accepts it as the spec requires.
		if (parsed.host) {
			throw invalidPathError(
				`toFile destination is not a local path: file URL host "${parsed.host}" names another machine`,
			);
		}

		try {
			return fileURLToPath(parsed);
		} catch (cause) {
			throw invalidPathError(
				`toFile destination is not a local path: ${cause.message}`,
			);
		}
	}
	if (typeof destination === "string") {
		return destination;
	}
	throw invalidPathError(
		"toFile destination must be a string path or file:// URL",
	);
}

/**
 * The MIME essence of a `Content-Type`: type and subtype, lower cased, parameters dropped.
 * @param {string | null} value
 * @returns {string}
 */
function mimeEssence(value) {
	if (!value) {
		return "";
	}
	return value.split(";", 1)[0].trim().toLowerCase();
}

/** ASCII whitespace, as Infra defines it, at either end of a string. */
const ASCII_WHITESPACE_ENDS = /^[\t\n\f\r ]+|[\t\n\f\r ]+$/g;

/** HTTP tab or space, the narrower set the fetch standard trims header elements with. */
const HTTP_TAB_OR_SPACE_ENDS = /^[\t ]+|[\t ]+$/g;

/** One ASCII whitespace code point, tested where the cursor stands. */
const ASCII_WHITESPACE = /[\t\n\f\r ]/;

/**
 * A leading floating-point number, by the HTML standard's rules for parsing floating-point number
 * values: leading whitespace and trailing junk are both allowed, and a value that does not begin
 * with a number does not match at all.
 */
const LEADING_FLOAT =
	/^[\t\n\f\r ]*([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)/;

/**
 * Collect code points from `input` at the cursor until one of `stops` is reached, advancing the
 * cursor to it. The building block the standards' own header parsing is written in terms of, so
 * the parsing below can follow their steps rather than approximate them.
 *
 * @param {string} input
 * @param {{ at: number }} cursor
 * @param {string} stops
 * @returns {string}
 */
function collectExcept(input, cursor, stops) {
	const start = cursor.at;
	while (cursor.at < input.length && !stops.includes(input[cursor.at])) {
		cursor.at++;
	}
	return input.slice(start, cursor.at);
}

/**
 * Collect an HTTP quoted string from `input` at the cursor, which stands on the opening quote.
 *
 * With `extract` set the string's contents are returned, its backslash escapes resolved; without
 * it the raw text, quotes and all, which is what splitting a header value needs so each element
 * it hands out still parses. One left unterminated runs to the end of the input.
 *
 * @param {string} input
 * @param {{ at: number }} cursor
 * @param {boolean} extract
 * @returns {string}
 */
function collectQuotedString(input, cursor, extract) {
	const start = cursor.at;
	let value = "";
	cursor.at++;

	for (;;) {
		value += collectExcept(input, cursor, '"\\');
		if (cursor.at >= input.length) {
			break;
		}

		const quoteOrBackslash = input[cursor.at++];
		if (quoteOrBackslash !== "\\") {
			break;
		}
		if (cursor.at >= input.length) {
			value += "\\";
			break;
		}
		value += input[cursor.at++];
	}

	return extract ? value : input.slice(start, cursor.at);
}

/**
 * Split a header value into its comma-separated elements, as the fetch standard's "get, decode,
 * and split" does: a comma inside a quoted string belongs to the string rather than ending an
 * element.
 *
 * @param {string} value
 * @returns {string[]}
 */
function splitHeaderValue(value) {
	const cursor = { at: 0 };
	const elements = [];
	let element = "";

	for (;;) {
		element += collectExcept(value, cursor, '",');
		if (value[cursor.at] === '"') {
			element += collectQuotedString(value, cursor, false);
			if (cursor.at < value.length) {
				continue;
			}
		}

		elements.push(element.replace(HTTP_TAB_OR_SPACE_ENDS, ""));
		element = "";
		if (cursor.at >= value.length) {
			return elements;
		}
		cursor.at++;
	}
}

/**
 * Parse a `dur` parameter into a duration in milliseconds.
 *
 * The Server-Timing specification defines `duration` against the HTML standard's floating-point
 * parse rather than the language's, so a value with trailing junk still reports the number it
 * starts with, and one that is not a number at all reports 0.
 *
 * @param {string} value
 * @returns {number}
 */
function parseDuration(value) {
	const match = LEADING_FLOAT.exec(value);
	return match ? Number(match[1]) : 0;
}

/**
 * Parse one `server-timing-metric` into a `PerformanceServerTiming`-shaped entry, following the
 * Server-Timing specification's own parse: the metric name runs to the first `;`, each parameter
 * name to its `=`, and a parameter value is either a quoted string or the text up to the next
 * `;`. Whitespace around each of those is not part of it, and characters that are part of no
 * parameter are ignored rather than ending the metric.
 *
 * Node has no `PerformanceServerTiming` class to mint, so the entry is a plain object carrying
 * the interface's three attributes, which is also what it serialises as.
 *
 * @param {string} field
 * @returns {{ name: string, duration: number, description: string } | null}
 */
function parseServerTimingMetric(field) {
	const cursor = { at: 0 };
	const name = collectExcept(field, cursor, ";").replace(
		ASCII_WHITESPACE_ENDS,
		"",
	);
	// A metric has to be named to be worth anything, and an empty name is the specification's
	// one parse failure.
	if (!name) {
		return null;
	}

	/** @type {Map<string, string>} */
	const params = new Map();
	while (cursor.at < field.length) {
		// The ";" the previous collection stopped at.
		cursor.at++;

		const param = collectExcept(field, cursor, "=").replace(
			ASCII_WHITESPACE_ENDS,
			"",
		);
		let value = "";
		if (field[cursor.at] === "=") {
			cursor.at++;
			while (
				cursor.at < field.length &&
				ASCII_WHITESPACE.test(field[cursor.at])
			) {
				cursor.at++;
			}

			if (field[cursor.at] === '"') {
				value = collectQuotedString(field, cursor, true);
				// The text between the closing quote and the next parameter is part of no
				// parameter, so it is read and dropped.
				collectExcept(field, cursor, ";");
			} else {
				value = collectExcept(field, cursor, ";").replace(
					ASCII_WHITESPACE_ENDS,
					"",
				);
			}
		}

		// A parameter named twice counts once, as the first of the two: a repeat is dropped
		// without disturbing the parameters that follow it.
		if (param && !params.has(param)) {
			params.set(param, value);
		}
	}

	return {
		name,
		duration: params.has("dur") ? parseDuration(params.get("dur")) : 0,
		description: params.get("desc") ?? "",
	};
}

/**
 * The metrics an origin reported in its `Server-Timing` header, in the order the header lists
 * them. A metric that does not parse is dropped and the rest of the list stands, because an
 * origin's diagnostics are worth more read in part than not at all.
 *
 * spec:RESP#request-timing
 *
 * @param {string | null} value
 * @returns {Array<{ name: string, duration: number, description: string }>}
 */
function parseServerTiming(value) {
	if (!value) {
		return [];
	}
	return splitHeaderValue(value)
		.map(parseServerTimingMetric)
		.filter((metric) => metric !== null);
}

/**
 * Mint the response's `PerformanceResourceTiming`.
 *
 * `PerformanceResourceTiming` is not constructible, so the only way to hand back the platform's
 * own type is `markResourceTiming`, which mints the entry and contributes it to the resource
 * timeline in the same call — the same route Node's own fetch takes. That is why this happens
 * once per request and is memoised: minting again would publish a second entry for the one
 * request.
 *
 * The platform's class implements an older revision of the interface, so the attributes it
 * lacks — and Faith's two additions — are defined on the entry as own properties, which shadow
 * the prototype's accessors and leave `instanceof` intact. The built-in `toJSON` only knows the
 * attributes the class carries, so it is shadowed too, or a serialised entry would silently
 * lose every field added here.
 *
 * spec:RESP#request-timing
 *
 * @param {Response} response
 * @param {number} fetchStart
 * @param {import('./index').TimingBreakdown} measurements
 * @returns {PerformanceResourceTiming}
 */
function mintResourceTiming(response, fetchStart, measurements) {
	const headersAt = fetchStart + measurements.headersMs;
	// A body that never finished has no last byte to report, which the interface spells 0.
	const responseEnd =
		measurements.bodyMs == null ? 0 : fetchStart + measurements.bodyMs;
	const cacheMode = measurements.fromCache ? "local" : "";

	const entry = performance.markResourceTiming(
		{
			startTime: fetchStart,
			endTime: responseEnd,
			finalServiceWorkerStartTime: 0,
			redirectStartTime: 0,
			redirectEndTime: 0,
			postRedirectStartTime: fetchStart,
			finalNetworkRequestStartTime: 0,
			finalNetworkResponseStartTime: headersAt,
			encodedBodySize: 0,
			decodedBodySize: 0,
			finalConnectionTimingInfo: {
				domainLookupStartTime: 0,
				domainLookupEndTime: 0,
				connectionStartTime: 0,
				connectionEndTime: 0,
				secureConnectionStartTime: 0,
				ALPNNegotiatedProtocol: measurements.nextHopProtocol,
			},
		},
		response.url,
		"fetch",
		globalThis,
		cacheMode,
		{},
		response.status,
		measurements.fromCache ? "cache" : "",
	);

	// Every attribute Faith states a value for, so the entry reads the same whatever revision
	// of the interface the platform's class happens to implement.
	const fields = {
		startTime: fetchStart,
		duration: responseEnd === 0 ? 0 : responseEnd - fetchStart,
		fetchStart,
		redirectStart: 0,
		redirectEnd: 0,
		workerStart: 0,
		workerRouterEvaluationStart: 0,
		workerCacheLookupStart: 0,
		workerMatchedRouterSource: "",
		workerFinalRouterSource: "",
		domainLookupStart: 0,
		domainLookupEnd: 0,
		connectStart: 0,
		connectEnd: 0,
		secureConnectionStart: 0,
		requestStart: 0,
		requestSent: 0,
		firstInterimResponseStart: 0,
		finalResponseHeadersStart: headersAt,
		responseStart: headersAt,
		responseEnd,
		nextHopProtocol: measurements.nextHopProtocol,
		deliveryType: measurements.fromCache ? "cache" : "",
		renderBlockingStatus: "non-blocking",
		responseStatus: response.status,
		contentType: mimeEssence(response.headers.get("content-type")),
		contentEncoding: measurements.contentEncoding ?? "",
		transferSize: 0,
		encodedBodySize: 0,
		decodedBodySize: 0,
		serverTiming: parseServerTiming(response.headers.get("server-timing")),
		reused: measurements.reused,
	};

	for (const [key, value] of Object.entries(fields)) {
		Object.defineProperty(entry, key, {
			value,
			enumerable: true,
			configurable: true,
		});
	}

	Object.defineProperty(entry, "toJSON", {
		value() {
			return {
				name: this.name,
				entryType: this.entryType,
				initiatorType: this.initiatorType,
				...fields,
			};
		},
		configurable: true,
	});

	return entry;
}

/**
 * Response class that provides spec-compliant Fetch API
 */
class Response {
	/** @type {import('./index').FaithResponse} */
	#nativeResponse;
	/** @type {Headers | undefined} */
	#headers;
	/** @type {ReadableStream<Uint8Array> | null | undefined} */
	#body;
	/**
	 * Whether a whole-body read (text/bytes/arrayBuffer/json/blob/toFile) has spent the body.
	 * These spend it without ever handing out the stream, so they close the `webResponse()`
	 * window too. Merely accessing `body` does not set this.
	 * @type {boolean}
	 */
	#bodyConsumed = false;
	/**
	 * The request's timing, shared with any clone: one request, one entry.
	 * @type {{ fetchStart: number, entry: Promise<PerformanceResourceTiming> | undefined }}
	 */
	#timing;

	constructor(nativeResponse, timing) {
		this.#nativeResponse = nativeResponse;
		this.#timing = timing;
	}

	// Mirror the native class's getters onto this prototype, once at class
	// definition time rather than per instance. The getter functions are
	// declared inside the class body so they can access #nativeResponse.
	static {
		const descriptors = Object.getOwnPropertyDescriptors(
			native.FaithResponse.prototype,
		);
		for (const [key, descriptor] of Object.entries(descriptors)) {
			if (descriptor.get && !(key in Response.prototype)) {
				Object.defineProperty(Response.prototype, key, {
					get() {
						return this.#nativeResponse[key];
					},
					enumerable: true,
					configurable: true,
				});
			}
		}
	}

	get headers() {
		if (!this.#headers) {
			const headers = new Headers();
			const headerPairs = this.#nativeResponse.headers();
			if (Array.isArray(headerPairs)) {
				for (const [name, value] of headerPairs) {
					headers.append(name, value);
				}
			}
			this.#headers = headers;
		}
		return this.#headers;
	}

	// spec:RESP#request-timing
	get timing() {
		this.#timing.entry ??= (async () => {
			const measurements = await this.#nativeResponse.timing();
			return mintResourceTiming(this, this.#timing.fetchStart, measurements);
		})();
		return this.#timing.entry;
	}

	get trailers() {
		return (async () => {
			const headerPairs = await this.#nativeResponse.trailers();
			if (!Array.isArray(headerPairs)) {
				return null;
			}

			const headers = new Headers();
			for (const [name, value] of headerPairs) {
				headers.append(name, value);
			}
			return headers;
		})();
	}

	// spec:BODY#the-body-stream
	get body() {
		// The native binding mints a fresh stream on every call, and each one
		// replays the body from the start. A response has one body stream, so
		// build it once and hand out the same object thereafter. `undefined`
		// means not built yet; `null` is a response that cannot carry a body.
		if (this.#body === undefined) {
			this.#body = this.#nativeResponse.body() ?? null;
		}
		return this.#body;
	}

	/**
	 * Convert response body to text (UTF-8)
	 * @returns {Promise<string>}
	 */
	async text() {
		const text = await this.#nativeResponse.text();
		this.#bodyConsumed = true;
		return text;
	}

	/**
	 * Get response body as bytes
	 * @returns {Promise<Uint8Array>}
	 */
	async bytes() {
		const bytes = await this.#nativeResponse.bytes();
		this.#bodyConsumed = true;
		return bytes;
	}

	/**
	 * Alias for bytes() that returns ArrayBuffer
	 * @returns {Promise<ArrayBuffer>}
	 */
	async arrayBuffer() {
		const buffer = await this.#nativeResponse.bytes();
		this.#bodyConsumed = true;
		// Slice out exactly this view's bytes: Buffers can be views into a
		// larger shared ArrayBuffer, so returning .buffer directly could leak
		// unrelated memory and have the wrong byteLength.
		return buffer.buffer.slice(
			buffer.byteOffset,
			buffer.byteOffset + buffer.byteLength,
		);
	}

	/**
	 * Parse response body as JSON
	 * @returns {Promise<any>}
	 */
	async json() {
		const value = await this.#nativeResponse.json();
		this.#bodyConsumed = true;
		return value;
	}

	/**
	 * Get response body as Blob
	 * @returns {Promise<Blob>}
	 */
	async blob() {
		const bytes = await this.#nativeResponse.bytes();
		this.#bodyConsumed = true;
		const contentType = this.headers.get("content-type") || "";
		return new Blob([bytes], { type: contentType });
	}

	/** Not supported. Will throw. */
	async formData() {
		throw new Error("not supported");
	}

	/**
	 * Write the response body straight to a file on disk, bypassing JavaScript. A whole-body
	 * read alongside `bytes()`: the first consumer wins and `integrity` is verified when set.
	 * @param {string | URL} destination A string path or `file://` URL to write to
	 * @param {{ overwrite?: boolean, mode?: number }} [options]
	 * @returns {Promise<{ path: string, bytesWritten: number }>}
	 * @throws {TypeError} `InvalidPath` if the destination does not name a local path
	 */
	toFile(destination, options) {
		// Resolve (and validate) the destination synchronously, so a bad path throws at the
		// call before the body is touched. The native write is the returned promise.
		const path = destinationPath(destination);

		// The callback travels as its own argument: a threadsafe function cannot be a field
		// of the native options object. The rest of the options carry through untouched.
		const { onProgress, ...rest } = options ?? {};
		if (onProgress !== undefined && typeof onProgress !== "function") {
			throw new TypeError("toFile onProgress must be a function");
		}

		// The body is spent once the write completes. An open failure leaves it undisturbed
		// and retryable, so only mark it consumed when the write resolves.
		return this.#nativeResponse.toFile(path, rest, onProgress).then((result) => {
			this.#bodyConsumed = true;
			return result;
		});
	}

	async discard() {
		return await this.#nativeResponse.discard();
	}

	/**
	 * Create a clone of the Response object
	 * @returns {Response} A new Response object with the same properties
	 * @throws {Error} If response body has already been read
	 */
	clone() {
		// The clone reads the same body over the same connection, so it shares the
		// original's timing rather than publishing a second entry for the one request.
		return new Response(this.#nativeResponse.clone(), this.#timing);
	}

	/**
	 * Convert to a Web API Response object
	 * @returns {Response} Web API Response object
	 * @throws {Error} If response body has been disturbed or Response constructor is not available
	 */
	webResponse() {
		// Check if Web API Response constructor is available
		if (typeof globalThis.Response !== "function") {
			throw new Error(
				"Web API Response constructor not available in this environment",
			);
		}

		// A whole-body read spends the body even though the stream was never handed out, so
		// the conversion is refused once one has happened (see toFile(), bytes(), and kin).
		if (this.#bodyConsumed) {
			throw disturbedResponseError();
		}

		// Otherwise build over the body stream. Accessing `body` without reading from it does
		// not stand in the way; a stream that has been read from or locked does, which the
		// Web `Response` constructor surfaces and we normalise to the already-disturbed error.
		try {
			return new globalThis.Response(this.body, {
				status: this.status,
				statusText: this.statusText,
				headers: this.headers,
			});
		} catch (cause) {
			throw disturbedResponseError(cause);
		}
	}
}

let defaultAgent;

/**
 * Fetch function wrapper
 * @param {string|Request|URL|{ toString(): string }} resource - The URL to fetch, a Request object, or an object with stringifier
 * @param {FetchOptions|Request} [options] - Fetch options (when resource is a Request, options override Request properties)
 * @returns {Promise<Response>}
 *
 * When a Request object is provided, all its properties (method, headers, body, mode, credentials,
 * cache, redirect, referrer, integrity, etc.) are extracted and passed to the native binding.
 * The options parameter can override any Request property.
 *
 * Objects with a toString() method (like URL objects) will have toString() called to get the URL string.
 *
 * Headers handling:
 * - Headers object: converted to array of [name, value] pairs
 * - Plain object: entries converted to array of [name, value] pairs
 * - null/undefined: treated as no headers
 * - Invalid types: throws TypeError
 */
async function fetch(resource, options = {}) {
	// The moment the request began, on the clock the platform's other performance entries
	// use, so the phases the native side measures can be placed against it
	// (spec:RESP#request-timing).
	const timing = { fetchStart: performance.now(), entry: undefined };
	let url;
	let nativeOptions;

	// Handle Request object as resource
	if (
		typeof resource === "object" &&
		resource !== null &&
		typeof resource.url === "string"
	) {
		// Extract url separately
		url = resource.url;

		// Copy all properties from Request object except url and bodyUsed
		const requestOptions = {};
		for (const key in resource) {
			if (key !== "url" && key !== "bodyUsed") {
				const value = resource[key];
				if (value !== undefined && value !== null) {
					requestOptions[key] = value;
				}
			}
		}

		// Handle body specially - Request.body is a ReadableStream that needs to be consumed
		if (requestOptions.body !== undefined && requestOptions.body !== null) {
			if (typeof resource.arrayBuffer === "function") {
				requestOptions.body = await resource.arrayBuffer();
			}
		}

		// Merge Request properties with options, options take precedence
		nativeOptions = { ...requestOptions, ...options };
	} else if (typeof resource === "string") {
		url = resource;
		nativeOptions = { ...options };
	} else if (resource && typeof resource.toString === "function") {
		// Handle objects with stringifier (like URL objects)
		url = resource.toString();
		nativeOptions = { ...options };
	} else {
		throw new TypeError(
			"First argument must be a string URL, Request object, or an object with a stringifier",
		);
	}

	// The fetch standard's Request constructor throws a TypeError when the method is GET or
	// HEAD and the body is non-null, and fetch() inherits that by constructing a Request. Faith
	// constructs no Request, so it enforces the rule here, before anything is sent
	// (spec:REQ#body). The method is matched case-insensitively, so a normalised `get` or `head`
	// in any case is refused just the same; an absent method defaults to GET. Any non-null body
	// counts, including an empty string.
	const requestMethod =
		nativeOptions.method == null ? "GET" : nativeOptions.method;
	if (
		typeof requestMethod === "string" &&
		(requestMethod.toUpperCase() === "GET" ||
			requestMethod.toUpperCase() === "HEAD") &&
		nativeOptions.body !== undefined &&
		nativeOptions.body !== null
	) {
		throw new TypeError(
			`Request with ${requestMethod.toUpperCase()} method cannot have a body`,
		);
	}

	// Convert headers to native format
	// This is the inverse of what Response does: Request headers go from
	// Headers/Object -> Array<[string, string]>, while Response headers go from
	// Array<[string, string]> -> Headers object
	if (nativeOptions.headers !== undefined && nativeOptions.headers !== null) {
		if (nativeOptions.headers instanceof Headers) {
			// Convert Headers object to array of tuples
			const headersArray = [];
			nativeOptions.headers.forEach((value, name) => {
				headersArray.push([name, value]);
			});
			nativeOptions.headers = headersArray;
		} else if (
			typeof nativeOptions.headers === "object" &&
			!Array.isArray(nativeOptions.headers)
		) {
			// Convert plain object to array of tuples
			const headersArray = [];
			for (const [name, value] of Object.entries(nativeOptions.headers)) {
				headersArray.push([name, value]);
			}
			nativeOptions.headers = headersArray;
		} else {
			throw new TypeError(
				"headers must be a Headers object or a plain object",
			);
		}
	} else if (nativeOptions.headers === null) {
		// Convert null to undefined so Rust treats it as None
		delete nativeOptions.headers;
	}

	// Convert body to Buffer if needed
	// Native binding handles: string, Buffer, Uint8Array
	// We convert: ArrayBuffer, Array<number>, ReadableStream, URLSearchParams
	// Validate ReadableStream bodies require duplex option
	if (nativeOptions.body !== undefined && nativeOptions.body !== null) {
		// Handle URLSearchParams
		if (nativeOptions.body instanceof URLSearchParams) {
			nativeOptions.body = nativeOptions.body.toString();
			// Set Content-Type if not already set (per Fetch spec)
			if (!nativeOptions.headers) {
				nativeOptions.headers = [];
			}
			const hasContentType = nativeOptions.headers.some(
				([name]) => name.toLowerCase() === "content-type",
			);
			if (!hasContentType) {
				nativeOptions.headers.push([
					"Content-Type",
					"application/x-www-form-urlencoded;charset=UTF-8",
				]);
			}
		}
		// Check if body is a ReadableStream
		else if (
			typeof nativeOptions.body === "object" &&
			typeof nativeOptions.body.getReader === "function"
		) {
			// ReadableStream body requires duplex option
			if (!nativeOptions.duplex) {
				throw new TypeError(
					"RequestInit's body is a ReadableStream and duplex option is not set",
				);
			}

			// Use our custom StreamBody to bypass NAPI-rs's buggy ReadableStream Reader.
			// We create a channel-based stream and push chunks from JavaScript,
			// which avoids the chunk dropping issue while preserving true streaming.
			const { body: streamBody, sender } = native.createStreamBodyPair();
			const originalStream = nativeOptions.body;
			delete nativeOptions.body;

			// Attach to the default agent if none is provided
			if (!nativeOptions.agent) {
				if (!defaultAgent) {
					defaultAgent = new native.Agent();
				}
				nativeOptions.agent = defaultAgent;
			}

			// Extract signal to pass as separate parameter
			const signal = nativeOptions.signal;
			delete nativeOptions.signal;

			// Check if signal is already aborted
			if (signal && signal.aborted) {
				sender.close();
				const error = new Error(
					"Aborted: the request was aborted before it could start",
				);
				error.name = "AbortError";
				error.code = ERROR_CODES.Aborted;
				throw error;
			}

			// Start the fetch with the StreamBody
			const responsePromise = faithFetch(
				url,
				nativeOptions,
				signal,
				streamBody,
			);

			// Pump chunks from the ReadableStream to the StreamBodySender
			const reader = originalStream.getReader();
			(async () => {
				try {
					while (true) {
						const { done, value } = await reader.read();
						if (done) {
							sender.close();
							break;
						}
						// Convert to Buffer if needed and push
						const buffer = Buffer.isBuffer(value)
							? value
							: Buffer.from(value);
						const sent = await sender.push(buffer);
						if (!sent) {
							// Receiver dropped (request completed/aborted)
							break;
						}
					}
				} catch (err) {
					// Stream error - close the sender
					sender.close();
				}
			})();

			const nativeResponse = await responsePromise;
			return new Response(nativeResponse, timing);
		} else if (nativeOptions.body instanceof ArrayBuffer) {
			nativeOptions.body = Buffer.from(nativeOptions.body);
		} else if (Array.isArray(nativeOptions.body)) {
			nativeOptions.body = Buffer.from(nativeOptions.body);
		}
	} else if (nativeOptions.body === null) {
		// Remove null body
		delete nativeOptions.body;
	}

	// Attach to the default agent if none is provided
	if (!nativeOptions.agent) {
		if (!defaultAgent) {
			defaultAgent = new native.Agent();
		}
		nativeOptions.agent = defaultAgent;
	}

	// Extract signal to pass as separate parameter
	const signal = nativeOptions.signal;
	delete nativeOptions.signal;

	// Check if signal is already aborted
	if (signal && signal.aborted) {
		const error = new Error(
			"Aborted: the request was aborted before it could start",
		);
		error.name = "AbortError";
		error.code = ERROR_CODES.Aborted;
		throw error;
	}

	const nativeResponse = await faithFetch(url, nativeOptions, signal, null);
	return new Response(nativeResponse, timing);
}

module.exports = {
	Agent: native.Agent,
	CacheMode: native.CacheMode,
	CacheStore: native.CacheStore,
	createStreamBodyPair: native.createStreamBodyPair,
	Credentials: native.CredentialsOption,
	Duplex: native.DuplexOption,
	ERROR_CODES,
	FAITH_VERSION: native.FAITH_VERSION,
	fetch,
	Http3Congestion: native.Http3Congestion,
	Redirect: native.Redirect,
	REQWEST_VERSION: native.REQWEST_VERSION,
	Response,
	StreamBody: native.StreamBody,
	StreamBodySender: native.StreamBodySender,
	USER_AGENT: native.USER_AGENT,
};
