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
  const ERR_RESPONSE_ALREADY_DISTURBED = native.errResponseAlreadyDisturbed();
  const ERR_RESPONSE_BODY_NOT_AVAILABLE = native.errResponseBodyNotAvailable();

  // Short error codes mapping exported from native for easier checks in JS and tests.
  // e.g. { invalid_header: "invalid_header", invalid_method: "invalid_method", ... }
  const ERROR_CODES = native.errorCodes();

  // helper map to attach .code based on FaithErrorKind being included in the native error message.
  const KIND_TO_CODE = {
    InvalidHeader: ERROR_CODES.invalid_header,
    InvalidMethod: ERROR_CODES.invalid_method,
    InvalidUrl: ERROR_CODES.invalid_url,
    InvalidCredentials: ERROR_CODES.invalid_credentials,
    InvalidOptions: ERROR_CODES.invalid_options,
    PermissionPolicy: ERROR_CODES.permission_policy,
    ResponseAlreadyDisturbed: ERROR_CODES.response_already_disturbed,
    ResponseBodyNotAvailable: ERROR_CODES.response_body_not_available,
    BodyStream: ERROR_CODES.body_stream_error,
    JsonParse: ERROR_CODES.json_parse_error,
    Timeout: ERROR_CODES.timeout,
    NetworkError: ERROR_CODES.network_error,
    RequestError: ERROR_CODES.request_error,
    Generic: ERROR_CODES.generic_failure,
  };

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
      try {
        return await this.#nativeResponse.text();
      } catch (error) {
        attachErrorCode(error);
        throw error;
      }
    }

    /**
     * Get response body as bytes
     * @returns {Promise<Uint8Array>}
     */
    async bytes() {
      try {
        return await this.#nativeResponse.bytes();
      } catch (error) {
        attachErrorCode(error);
        throw error;
      }
    }

    /**
     * Alias for bytes() that returns ArrayBuffer
     * @returns {Promise<ArrayBuffer>}
     */
    async arrayBuffer() {
      try {
        return (await this.bytes()).buffer;
      } catch (error) {
        // `bytes()` already attached a .code where applicable, so just rethrow.
        throw error;
      }
    }

    /**
     * Parse response body as JSON
     * @returns {Promise<any>}
     */
    async json() {
      try {
        return await this.#nativeResponse.json();
      } catch (error) {
        attachErrorCode(error);
        throw error;
      }
    }

    /**
     * Get response body as Blob
     * @returns {Promise<Blob>}
     */
    async blob() {
      try {
        const bytes = await this.#nativeResponse.bytes();
        const contentType = this.headers.get("content-type") || "";
        return new Blob([bytes], { type: contentType });
      } catch (error) {
        attachErrorCode(error);
        throw error;
      }
    }

    /**
     * Create a clone of the Response object
     * @returns {Response} A new Response object with the same properties
     * @throws {Error} If response body has already been read
     */
    clone() {
      try {
        return new Response(this.#nativeResponse.clone());
      } catch (error) {
        attachErrorCode(error);
        throw error;
      }
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
        const err = new Error(ERR_RESPONSE_BODY_NOT_AVAILABLE);
        // Attach the canonical code to the thrown error using a safe property definition
        try {
          Object.defineProperty(err, "code", {
            value: ERROR_CODES.response_body_not_available,
            enumerable: true,
            configurable: true,
            writable: true,
          });
        } catch (e) {
          // fallback: attempt direct assignment
          try {
            err.code = ERROR_CODES.response_body_not_available;
          } catch (e) {}
        }
        throw err;
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

    try {
      const nativeResponse = await faithFetch(url, nativeOptions);
      return new Response(nativeResponse);
    } catch (error) {
      // If the native error contains a FaithErrorKind prefix (e.g. "InvalidHeader: ..."),
      // attach a consistent `.code` property to make tests and user code easier to deal with.
      attachErrorCode(error);
      throw error;
    }
  }

  return {
    Response,
    fetch,
  };
}

module.exports = { createWrapper };
