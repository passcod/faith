/**
 * Faith Fetch API Wrapper TypeScript Definitions
 *
 * This provides TypeScript definitions for the spec-compliant Fetch API wrapper.
 */

export interface FetchOptions {
  method?: string;
  headers?: Record<string, string>;
  body?: Array<number> | string | ArrayBuffer | Uint8Array;
  timeout?: number;
}

export class Response {
  readonly status: number;
  readonly statusText: string;
  readonly headers: Record<string, string>;
  readonly ok: boolean;
  readonly url: string;
  readonly redirected: boolean;
  readonly timestamp: number;
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

/**
 * Native bindings for advanced use
 */
export declare const native: {
  FetchResponse: any;
  fetch: any;
};
