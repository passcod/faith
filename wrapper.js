/**
 * Faith Fetch API Wrapper
 *
 * This wrapper provides a spec-compliant Fetch API interface on top of
 * the native Rust bindings. The main difference is that `body` is exposed
 * as a property/getter instead of a method, and the class is named `Response`
 * instead of `FetchResponse`.
 */

const native = require("./index.js");
const { faithFetch } = native;

// Generate ERROR_CODES const enum from native error codes
// e.g. { InvalidHeader: "InvalidHeader", InvalidMethod: "InvalidMethod", ... }
const ERROR_CODES = native.errorCodes().reduce((acc, code) => {
	acc[code] = code;
	return acc;
}, {});

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
 * lacks — and Fáith's two additions — are defined on the entry as own properties, which shadow
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

	// Every attribute Fáith states a value for, so the entry reads the same whatever revision
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
		serverTiming: [],
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
		return await this.#nativeResponse.text();
	}

	/**
	 * Get response body as bytes
	 * @returns {Promise<Uint8Array>}
	 */
	async bytes() {
		return await this.#nativeResponse.bytes();
	}

	/**
	 * Alias for bytes() that returns ArrayBuffer
	 * @returns {Promise<ArrayBuffer>}
	 */
	async arrayBuffer() {
		const buffer = await this.#nativeResponse.bytes();
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
		return await this.#nativeResponse.json();
	}

	/**
	 * Get response body as Blob
	 * @returns {Promise<Blob>}
	 */
	async blob() {
		const bytes = await this.#nativeResponse.bytes();
		const contentType = this.headers.get("content-type") || "";
		return new Blob([bytes], { type: contentType });
	}

	/** Not supported. Will throw. */
	async formData() {
		throw new Error("not supported");
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

		// Create and return a Web API Response object
		return new globalThis.Response(this.body, {
			status: this.status,
			statusText: this.statusText,
			headers: this.headers,
		});
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
