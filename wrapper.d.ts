/**
 * Faith Fetch API Wrapper TypeScript Definitions
 *
 * This provides TypeScript definitions for the spec-compliant Fetch API wrapper.
 */

/**
 * Error codes const enum
 *
 * NOTE: This must be kept in sync with FaithErrorKind in src/error.rs
 * Run `npm test` to validate sync (test/error-codes.test.js checks this)
 */
export const ERROR_CODES: {
  readonly InvalidHeader: "InvalidHeader";
  readonly InvalidMethod: "InvalidMethod";
  readonly InvalidUrl: "InvalidUrl";
  readonly InvalidCredentials: "InvalidCredentials";
  readonly InvalidOptions: "InvalidOptions";
  readonly BlockedByPolicy: "BlockedByPolicy";
  readonly ResponseAlreadyDisturbed: "ResponseAlreadyDisturbed";
  readonly ResponseBodyNotAvailable: "ResponseBodyNotAvailable";
  readonly BodyStream: "BodyStream";
  readonly JsonParse: "JsonParse";
  readonly Utf8Parse: "Utf8Parse";
  readonly Timeout: "Timeout";
  readonly PermissionPolicy: "PermissionPolicy";
  readonly Network: "Network";
  readonly RuntimeThread: "RuntimeThread";
  readonly Generic: "Generic";
};

export interface FetchOptions {
  method?: string;
  headers?: Record<string, string> | Headers;
  body?: string | Buffer | Uint8Array | Array<number> | ArrayBuffer;
  timeout?: number;
}

export class Response {
  readonly status: number;
  readonly statusText: string;
  readonly headers: Headers;
  readonly ok: boolean;
  readonly url: string;
  readonly redirected: boolean;
  readonly bodyUsed: boolean;

  /**
   * Get the response body as a ReadableStream
   * This is a getter to match the Fetch API spec
   */
  readonly body: ReadableStream<Uint8Array> | null;

  /**
   * Convert response body to text (UTF-8)
   * @returns Promise that resolves with the response body as text
   */
  text(): Promise<string>;

  /**
   * Get response body as bytes
   * @returns Promise that resolves with the response body as Uint8Array
   */
  bytes(): Promise<Uint8Array>;

  /**
   * Get response body as ArrayBuffer
   * @returns Promise that resolves with the response body as ArrayBuffer
   */
  arrayBuffer(): Promise<ArrayBuffer>;

  /**
   * Parse response body as JSON
   * @returns Promise that resolves with the parsed JSON data
   */
  json(): Promise<any>;

  /**
   * Get response body as Blob
   * @returns Promise that resolves with the response body as Blob
   */
  blob(): Promise<Blob>;

  /**
   * Create a clone of the Response object
   * @returns A new Response object with the same properties
   * @throws If response body has already been read
   */
  clone(): Response;

  /**
   * Convert to a Web API Response object
   * @returns Web API Response object
   * @throws If response body has been disturbed
   */
  webResponse(): Response;
}

/**
 * Fetch function
 * @param url - The URL to fetch
 * @param options - Fetch options
 * @returns Promise that resolves with a FetchResponse
 */
export declare function fetch(
  url: string,
  options?: FetchOptions,
): Promise<Response>;
