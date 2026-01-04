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
 * Response class that provides spec-compliant Fetch API
 */
class Response {
	/** @type {import('./index').FaithResponse} */
	#nativeResponse;

	constructor(nativeResponse) {
		this.#nativeResponse = nativeResponse;

		const nativeProto = Object.getPrototypeOf(this.#nativeResponse);
		const descriptors = Object.getOwnPropertyDescriptors(nativeProto);

		for (const [key, descriptor] of Object.entries(descriptors)) {
			if (descriptor.get) {
				Object.defineProperty(this, key, {
					get: () => this.#nativeResponse[key],
					enumerable: true,
					configurable: true,
				});
			}
		}
	}

	get headers() {
		const headers = new Headers();
		const headerPairs = this.#nativeResponse.headers();
		if (Array.isArray(headerPairs)) {
			for (const [name, value] of headerPairs) {
				headers.append(name, value);
			}
		}
		return headers;
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

	get body() {
		return this.#nativeResponse.body();
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
		return buffer.buffer;
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
		return new Response(this.#nativeResponse.clone());
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

			// Consume the ReadableStream into a Buffer
			const reader = nativeOptions.body.getReader();
			const chunks = [];
			try {
				while (true) {
					const { done, value } = await reader.read();
					if (done) break;
					chunks.push(value);
				}
			} finally {
				reader.releaseLock();
			}

			// Concatenate all chunks into a single Buffer
			const totalLength = chunks.reduce(
				(acc, chunk) => acc + chunk.length,
				0,
			);
			const result = new Uint8Array(totalLength);
			let offset = 0;
			for (const chunk of chunks) {
				result.set(chunk, offset);
				offset += chunk.length;
			}
			nativeOptions.body = Buffer.from(result);
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

	const nativeResponse = await faithFetch(url, nativeOptions, signal);
	return new Response(nativeResponse);
}

module.exports = {
	Agent: native.Agent,
	CacheMode: native.CacheMode,
	CacheStore: native.CacheStore,
	Credentials: native.CredentialsOption,
	Duplex: native.DuplexOption,
	ERROR_CODES,
	FAITH_VERSION: native.FAITH_VERSION,
	fetch,
	Http3Congestion: native.Http3Congestion,
	Redirect: native.Redirect,
	REQWEST_VERSION: native.REQWEST_VERSION,
	Response,
	USER_AGENT: native.USER_AGENT,
};
