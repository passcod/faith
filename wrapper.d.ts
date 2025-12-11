/**
 * Faith Fetch API Wrapper TypeScript Definitions
 *
 * This provides TypeScript definitions for the spec-compliant Fetch API wrapper.
 */

export {
	Agent,
	AgentHttp3Options,
	AgentOptions,
	AgentStats,
	Header,
	Http3Congestion,
	FAITH_VERSION,
	REQWEST_VERSION,
	USER_AGENT,
} from "./index";

/**
 * Error codes const enum
 *
 * NOTE: This must be kept in sync with FaithErrorKind in src/error.rs
 * Run `npm test` to validate sync (test/error-codes.test.js checks this)
 */
export const ERROR_CODES: {
	readonly Aborted: "Aborted";
	readonly BodyStream: "BodyStream";
	readonly InvalidHeader: "InvalidHeader";
	readonly InvalidMethod: "InvalidMethod";
	readonly InvalidUrl: "InvalidUrl";
	readonly JsonParse: "JsonParse";
	readonly Network: "Network";
	readonly ResponseAlreadyDisturbed: "ResponseAlreadyDisturbed";
	readonly ResponseBodyNotAvailable: "ResponseBodyNotAvailable";
	readonly RuntimeThread: "RuntimeThread";
	readonly Timeout: "Timeout";
	readonly Utf8Parse: "Utf8Parse";
};

export interface FetchOptions {
	method?: string;
	headers?: Record<string, string> | Headers;
	body?: string | Buffer | Uint8Array | Array<number> | ArrayBuffer;
	timeout?: number; // milliseconds
	credentials?: "omit" | "same-origin" | "include";
	duplex?: "half";
	signal?: AbortSignal;
	agent?: Agent;
}

export interface PeerInformation {
	address?: string;
	certificate?: Buffer;
}

export class Response {
	readonly bodyUsed: boolean;
	readonly headers: Headers;
	readonly ok: boolean;
	readonly peer: PeerInformation;
	readonly redirected: boolean;
	readonly status: number;
	readonly statusText: string;
	readonly type: "basic";
	readonly url: string;
	readonly version: string;

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

	/** Not supported. Will throw. */
	formData(): Promise<FormData>;

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
	webResponse(): globalThis.Response;
}

/**
 * Fetch function
 * @param input - The URL to fetch, a Request object, URL object, or any object with a toString() method
 * @param options - Fetch options (overrides Request properties when provided)
 * @returns Promise that resolves with a Response
 */
export declare function fetch(
	input: string | Request | URL | { toString(): string },
	options?: FetchOptions,
): Promise<Response>;
