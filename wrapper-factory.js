/**
 * Faith Fetch API Wrapper Factory
 *
 * This module provides a factory function to create spec-compliant Fetch API wrappers
 * around the native faith bindings. It contains the shared logic used by both
 * Node.js and browser wrappers.
 */

/**
 * Creates a Response class wrapper around native FetchResponse
 * @param {import('./index')} native - Native bindings object with FetchResponse and fetch
 * @returns {Object} Object containing Response class and fetch function
 */
function createWrapper(native) {
  const { faithFetch } = native;
  // Canonical error messages from native module
  // These functions are exported by the native module (Rust) as helpers.
  const ERR_RESPONSE_ALREADY_DISTURBED =
    native.err_response_already_disturbed();
  const ERR_RESPONSE_BODY_NOT_AVAILABLE =
    native.err_response_body_not_available();

  /**
   * Response class that provides spec-compliant Fetch API
   */
  class Response {
    /** @type {import('./index').FaithResponse} */
    #nativeResponse;
    /** @type {ReadableStream<Buffer>?} */
    #bodyStream = null;

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

      // Copy getters from native response
      Object.defineProperties(this, {
        status: {
          get: () => this.#nativeResponse.status,
          enumerable: true,
          configurable: true,
        },
        statusText: {
          get: () => this.#nativeResponse.statusText,
          enumerable: true,
          configurable: true,
        },
        headers: {
          get: () => headers,
          enumerable: true,
          configurable: true,
        },
        ok: {
          get: () => this.#nativeResponse.ok,
          enumerable: true,
          configurable: true,
        },
        url: {
          get: () => this.#nativeResponse.url,
          enumerable: true,
          configurable: true,
        },
        redirected: {
          get: () => this.#nativeResponse.redirected,
          enumerable: true,
          configurable: true,
        },
        timestamp: {
          get: () => this.#nativeResponse.timestamp,
          enumerable: true,
          configurable: true,
        },
        bodyUsed: {
          get: () => this.#nativeResponse.bodyUsed,
          enumerable: true,
          configurable: true,
        },
      });
    }

    /**
     * Get the response body as a ReadableStream
     * This is a getter to match the Fetch API spec
     */
    get body() {
      if (this.#nativeResponse.bodyEmpty) {
        return null;
      }

      if (this.#bodyStream !== null) {
        return this.#bodyStream;
      }

      return (this.#bodyStream = this.#nativeResponse.body());
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
      return (await this.bytes()).buffer;
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

      // Get the body stream
      const bodyStream = this.body;
      if (bodyStream === null) {
        throw new Error(ERR_RESPONSE_BODY_NOT_AVAILABLE);
      }

      // Create and return a Web API Response object
      return new globalThis.Response(bodyStream, {
        status: this.status,
        statusText: this.statusText,
        headers: this.headers,
      });
    }
  }

  /**
   * Fetch function wrapper
   * @param {string} url - The URL to fetch
   * @param {Object} [options] - Fetch options
   * @param {string} [options.method] - HTTP method
   * @param {Object|Headers} [options.headers] - HTTP headers (either Headers object or plain object)
   * @param {Buffer|Array<number>|string|ArrayBuffer|Uint8Array} [options.body] - Request body
   * @param {number} [options.timeout] - Timeout in seconds
   * @returns {Promise<Response>}
   */
  async function fetch(url, options = {}) {
    // Convert options to match native API
    const nativeOptions = { ...options };

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
    if (nativeOptions.body !== undefined) {
      if (typeof nativeOptions.body === "string") {
        // Convert string to Buffer
        nativeOptions.body = Buffer.from(nativeOptions.body);
      } else if (nativeOptions.body instanceof ArrayBuffer) {
        // Convert ArrayBuffer to Buffer
        nativeOptions.body = Buffer.from(nativeOptions.body);
      } else if (nativeOptions.body instanceof Uint8Array) {
        // Convert Uint8Array to Buffer
        nativeOptions.body = Buffer.from(nativeOptions.body);
      } else if (Array.isArray(nativeOptions.body)) {
        // Convert Array<number> to Buffer
        nativeOptions.body = Buffer.from(nativeOptions.body);
      }
      // If it's already a Buffer, keep as is
    }

    const nativeResponse = await faithFetch(url, nativeOptions);
    return new Response(nativeResponse);
  }

  return {
    Response,
    fetch,
  };
}

module.exports = { createWrapper };
