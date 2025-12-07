/**
 * Faith Fetch API Wrapper Factory
 *
 * This module provides a factory function to create spec-compliant Fetch API wrappers
 * around the native faith bindings. It contains the shared logic used by both
 * Node.js and browser wrappers.
 */

/**
 * Creates a Response class wrapper around native FetchResponse
 * @param {Object} native - Native bindings object with FetchResponse and fetch
 * @returns {Object} Object containing Response class and fetch function
 */
function createWrapper(native) {
  const { FetchResponse: NativeFetchResponse, fetch: nativeFetch } = native;

  /**
   * Response class that provides spec-compliant Fetch API
   */
  class Response {
    #nativeResponse;
    #bodyStream = null;
    #bodyUsed = false;
    #bodyAccessed = false;

    constructor(nativeResponse) {
      this.#nativeResponse = nativeResponse;

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
          get: () => this.#nativeResponse.headers,
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
          get: () =>
            this.#bodyUsed ||
            this.#bodyAccessed ||
            this.#nativeResponse.bodyUsed,
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
      // If body is already used (disturbed), return null
      if (this.#bodyUsed) {
        return null;
      }

      // If we already have a stream, return it
      if (this.#bodyStream !== null) {
        return this.#bodyStream;
      }

      // Mark that body has been accessed
      this.#bodyAccessed = true;

      // Get the stream from the native response
      const stream = this.#nativeResponse.body();
      if (stream) {
        this.#bodyStream = stream;
      } else {
        // If native returns null, body has been disturbed
        this.#bodyUsed = true;
      }

      return stream;
    }

    /**
     * Convert response body to text (UTF-8)
     * @returns {Promise<string>}
     */
    async text() {
      if (this.#bodyUsed) {
        throw new Error("Response already disturbed");
      }

      // If body was accessed (stream created), we can't use text()
      if (this.#bodyAccessed) {
        throw new Error("Response already disturbed");
      }

      this.#bodyUsed = true;
      this.#bodyStream = null; // Can't have both stream and consumed body

      try {
        return await this.#nativeResponse.text();
      } catch (error) {
        // If native throws "Response already disturbed", it means body() was called
        if (error.message.includes("disturbed")) {
          throw new Error("Response already disturbed");
        }
        throw error;
      }
    }

    /**
     * Get response body as bytes
     * @returns {Promise<Uint8Array>}
     */
    async bytes() {
      if (this.#bodyUsed) {
        throw new Error("Response already disturbed");
      }

      // If body was accessed (stream created), we can't use bytes()
      if (this.#bodyAccessed) {
        throw new Error("Response already disturbed");
      }

      this.#bodyUsed = true;
      this.#bodyStream = null; // Can't have both stream and consumed body

      try {
        const bytesArray = await this.#nativeResponse.bytes();
        return new Uint8Array(bytesArray);
      } catch (error) {
        // If native throws "Response already disturbed", it means body() was called
        if (error.message.includes("disturbed")) {
          throw new Error("Response already disturbed");
        }
        throw error;
      }
    }

    /**
     * Alias for bytes() that returns ArrayBuffer
     * @returns {Promise<ArrayBuffer>}
     */
    async arrayBuffer() {
      const bytes = await this.bytes();
      return bytes.buffer;
    }
  }

  /**
   * Fetch function wrapper
   * @param {string} url - The URL to fetch
   * @param {Object} [options] - Fetch options
   * @param {string} [options.method] - HTTP method
   * @param {Object} [options.headers] - HTTP headers
   * @param {Array<number>|string|ArrayBuffer|Uint8Array} [options.body] - Request body
   * @param {number} [options.timeout] - Timeout in seconds
   * @returns {Promise<Response>}
   */
  async function fetch(url, options = {}) {
    // Convert options to match native API
    const nativeOptions = { ...options };

    // Convert body to Array<number> if needed
    if (nativeOptions.body !== undefined) {
      if (typeof nativeOptions.body === "string") {
        // Convert string to bytes
        const encoder = new TextEncoder();
        nativeOptions.body = Array.from(encoder.encode(nativeOptions.body));
      } else if (nativeOptions.body instanceof ArrayBuffer) {
        // Convert ArrayBuffer to bytes
        nativeOptions.body = Array.from(new Uint8Array(nativeOptions.body));
      } else if (nativeOptions.body instanceof Uint8Array) {
        // Convert Uint8Array to bytes
        nativeOptions.body = Array.from(nativeOptions.body);
      }
      // If it's already Array<number>, keep as is
    }

    const nativeResponse = await nativeFetch(url, nativeOptions);
    return new Response(nativeResponse);
  }

  return {
    Response,
    fetch,
    // Also expose native bindings for advanced use
    native,
  };
}

module.exports = { createWrapper };
