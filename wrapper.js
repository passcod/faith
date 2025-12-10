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

    // Create a Headers object from the array of header pairs
    const headers = new Headers();
    const headerPairs = this.#nativeResponse.headers;
    if (Array.isArray(headerPairs)) {
      for (const [name, value] of headerPairs) {
        headers.append(name, value);
      }
    }

    Object.defineProperty(this, "headers", {
      get: () => headers,
      enumerable: true,
      configurable: true,
    });

    const nativeProto = Object.getPrototypeOf(this.#nativeResponse);
    const descriptors = Object.getOwnPropertyDescriptors(nativeProto);

    for (const [key, descriptor] of Object.entries(descriptors)) {
      if (descriptor.get && key !== "headers") {
        Object.defineProperty(this, key, {
          get: () => this.#nativeResponse[key],
          enumerable: true,
          configurable: true,
        });
      }
    }
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
 * @param {string|Request} input - The URL to fetch or a Request object
 * @param {Object} [options] - Fetch options (when input is a Request, options override Request properties)
 * @param {string} [options.method] - HTTP method
 * @param {Object|Headers} [options.headers] - HTTP headers (either Headers object or plain object)
 * @param {Buffer|Array<number>|string|ArrayBuffer|Uint8Array} [options.body] - Request body
 * @param {number} [options.timeout] - Timeout in seconds
 * @returns {Promise<Response>}
 *
 * When a Request object is provided, all its properties (method, headers, body, mode, credentials,
 * cache, redirect, referrer, integrity, etc.) are extracted and passed to the native binding.
 * The options parameter can override any Request property.
 */
async function fetch(input, options = {}) {
  let url;
  let nativeOptions;

  // Handle Request object as input
  if (
    typeof input === "object" &&
    input !== null &&
    typeof input.url === "string"
  ) {
    // Extract url separately
    url = input.url;

    // Copy all properties from Request object except url and bodyUsed
    const requestOptions = {};
    for (const key in input) {
      if (key !== "url" && key !== "bodyUsed") {
        const value = input[key];
        if (value !== undefined && value !== null) {
          requestOptions[key] = value;
        }
      }
    }

    // Handle body specially - Request.body is a ReadableStream that needs to be consumed
    if (requestOptions.body !== undefined && requestOptions.body !== null) {
      if (typeof input.arrayBuffer === "function") {
        requestOptions.body = await input.arrayBuffer();
      }
    }

    // Merge Request properties with options, options take precedence
    nativeOptions = { ...requestOptions, ...options };
  } else if (typeof input === "string") {
    url = input;
    nativeOptions = { ...options };
  } else {
    throw new TypeError(
      "First argument must be a string URL or Request object",
    );
  }

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
      throw new TypeError("headers must be a Headers object or a plain object");
    }
  } else if (nativeOptions.headers === null) {
    // Convert null to undefined so Rust treats it as None
    delete nativeOptions.headers;
  }

  // Convert body to Buffer if needed
  // Native binding handles: string, Buffer, Uint8Array
  // We convert: ArrayBuffer, Array<number>
  if (nativeOptions.body !== undefined) {
    if (nativeOptions.body instanceof ArrayBuffer) {
      nativeOptions.body = Buffer.from(nativeOptions.body);
    } else if (Array.isArray(nativeOptions.body)) {
      nativeOptions.body = Buffer.from(nativeOptions.body);
    }
  }

  // Attach to the default agent if none is provided
  if (!nativeOptions.agent) {
    if (!defaultAgent) {
      defaultAgent = new native.FaithAgent();
    }
    nativeOptions.agent = defaultAgent;
  }

  const nativeResponse = await faithFetch(url, nativeOptions);
  return new Response(nativeResponse);
}

module.exports = {
  Agent: native.FaithAgent,
  ERROR_CODES,
  FAITH_VERSION: native.FAITH_VERSION,
  fetch,
  REQWEST_VERSION: native.REQWEST_VERSION,
  Response,
  USER_AGENT: native.USER_AGENT,
};
