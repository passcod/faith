import { Agent } from "./index";
export {
	Agent,
	AgentCacheOptions,
	AgentDnsOptions,
	AgentHttp3Options,
	AgentPoolOptions,
	AgentTimeoutOptions,
	AgentTlsOptions,
	AgentOptions,
	AgentStats,
	CacheMode,
	CacheStore,
	CredentialsOption as Credentials,
	DnsOverride,
	DuplexOption as Duplex,
	Header,
	Http3Congestion,
	Redirect,
	FAITH_VERSION,
	REQWEST_VERSION,
	USER_AGENT,
} from "./index";

// NOTE: This must be kept in sync with FaithErrorKind in src/error.rs
// Run `npm test` to validate sync (test/error-codes.test.js checks this)
export const ERROR_CODES: {
	readonly Aborted: "Aborted";
	readonly AddressParse: "AddressParse";
	readonly BodyStream: "BodyStream";
	readonly Config: "Config";
	readonly IntegrityMismatch: "IntegrityMismatch";
	readonly InvalidHeader: "InvalidHeader";
	readonly InvalidIntegrity: "InvalidIntegrity";
	readonly InvalidMethod: "InvalidMethod";
	readonly InvalidUrl: "InvalidUrl";
	readonly JsonParse: "JsonParse";
	readonly Network: "Network";
	readonly PemParse: "PemParse";
	readonly Redirect: "Redirect";
	readonly ResponseAlreadyDisturbed: "ResponseAlreadyDisturbed";
	readonly ResponseBodyNotAvailable: "ResponseBodyNotAvailable";
	readonly RuntimeThread: "RuntimeThread";
	readonly Timeout: "Timeout";
	readonly Utf8Parse: "Utf8Parse";
};

export interface FetchOptions {
	/**
	 * This is custom to Fáith.
	 *
	 * You can create an `Agent`, and pass it here to have the request executed by the `Agent`. See the
	 * documentation for the `Agent` options you can set with this, and the agent data you can access.
	 * Notably an agent has a DNS cache, and may be configured to handle cookies and/or an HTTP cache.
	 *
	 * When not provided, a global default `Agent` is created on first use.
	 */
	agent?: Agent;
	/**
	 * The request body contains content to send to the server, for example in a `POST` or `PUT` request.
	 * It is specified as an instance of any of the following types:
	 *
	 * - a string
	 * - `ArrayBuffer`
	 * - `Blob`
	 * - `DataView`
	 * - `File`
	 * - `FormData`
	 * - `TypedArray`
	 * - ~~`URLSearchParams`~~ Not yet implemented.
	 * - `ReadableStream` Note that Fáith currently reads this into memory before sending the request.
	 *
	 * If `body` is a `ReadableStream`, the `duplex` option must also be set.
	 */
	body?: string | Buffer | Uint8Array | Array<number> | ArrayBuffer;
	/**
	 * The cache mode you want to use for the request. This may be any one of the following values:
	 *
	 * - `default`: The client looks in its HTTP cache for a response matching the request.
	 *   - If there is a match and it is fresh, it will be returned from the cache.
	 *   - If there is a match but it is stale, the client will make a conditional request to the remote
	 *     server. If the server indicates that the resource has not changed, it will be returned from the
	 *     cache. Otherwise the resource will be downloaded from the server and the cache will be updated.
	 *   - If there is no match, the client will make a normal request, and will update the cache with
	 *     the downloaded resource.
	 *
	 * - `no-store`: The client fetches the resource from the remote server without first looking in the
	 *   cache, and will not update the cache with the downloaded resource.
	 *
	 * - `reload`: The client fetches the resource from the remote server without first looking in the
	 *   cache, but then will update the cache with the downloaded resource.
	 *
	 * - `no-cache`: The client looks in its HTTP cache for a response matching the request.
	 *   - If there is a match, fresh or stale, the client will make a conditional request to the remote
	 *     server. If the server indicates that the resource has not changed, it will be returned from the
	 *     cache. Otherwise the resource will be downloaded from the server and the cache will be updated.
	 *   - If there is no match, the client will make a normal request, and will update the cache with
	 *     the downloaded resource.
	 *
	 * - `force-cache`: The client looks in its HTTP cache for a response matching the request.
	 *   - If there is a match, fresh or stale, it will be returned from the cache.
	 *   - If there is no match, the client will make a normal request, and will update the cache with
	 *     the downloaded resource.
	 *
	 * - `only-if-cached`: The client looks in its HTTP cache for a response matching the request.
	 *   - If there is a match, fresh or stale, it will be returned from the cache.
	 *   - If there is no match, a network error is returned.
	 *
	 * - `ignore-rules`: Custom to Fáith. Overrides the check that determines if a response can be cached
	 *   to always return true on 200. Uses any response in the HTTP cache matching the request, not
	 *   paying attention to staleness. If there was no response, it creates a normal request and updates
	 *   the HTTP cache with the response.
	 */
	cache?:
		| "default"
		| "force-cache"
		| "ignore-rules"
		| "no-cache"
		| "no-store"
		| "only-if-cached"
		| "reload";
	/**
	 * Controls whether or not the client sends credentials with the request, as well as whether any
	 * `Set-Cookie` response headers are respected. Credentials are cookies, ~~TLS client certificates,~~
	 * or authentication headers containing a username and password. This option may be any one of the
	 * following values:
	 *
	 * - `omit`: Never send credentials in the request or include credentials in the response.
	 * - ~~`same-origin`~~: Fáith does not implement this, as there is no concept of "origin" on the server.
	 * - `include`: Always include credentials, ~~even for cross-origin requests.~~
	 *
	 * Fáith ignores the `Access-Control-Allow-Credentials` and `Access-Control-Allow-Origin` headers.
	 *
	 * Fáith currently does not `omit` the TLS client certificate when the request's `Agent` has one
	 * configured. This is an upstream limitation.
	 *
	 * If the request's `Agent` has cookies enabled, new cookies from the response will be added to the
	 * cookie jar, even as Fáith strips them from the request and response headers returned to the user.
	 * This is an upstream limitation.
	 *
	 * Defaults to `include` (browsers default to `same-origin`).
	 */
	credentials?: "omit" | "same-origin" | "include";
	/**
	 * Controls duplex behavior of the request. If this is present it must have the value `half`, meaning
	 * that Fáith will send the entire request before processing the response.
	 *
	 * This option must be present when `body` is a `ReadableStream`.
	 */
	duplex?: "half";
	/**
	 * Any headers you want to add to your request, contained within a `Headers` object or an object
	 * literal whose keys are the names of headers and whose values are the header values.
	 *
	 * Fáith allows all request headers to be set (unlike browsers, which [forbid][1] a number of them).
	 *
	 * [1]: https://developer.mozilla.org/en-US/docs/Glossary/Forbidden_request_header
	 */
	headers?: Record<string, string> | Headers;
	/**
	 * The request method. Defaults to `GET`.
	 */
	method?: string;
	/**
	 * An `AbortSignal`. If this option is set, the request can be canceled by calling `abort()` on the
	 * corresponding `AbortController`.
	 */
	signal?: AbortSignal;
	/**
	 * Custom to Fáith. Cancels the request after this many milliseconds.
	 *
	 * This will give a different error to using `signal` with a timeout, which might be preferable in
	 * some cases. It also has a slightly different internal behaviour: `signal` may abort the request
	 * only until the response headers have been received, while `timeout` will apply through the entire
	 * response receipt.
	 */
	timeout?: number;
}

export interface PeerInformation {
	/**
	 * The IP address and port of the peer, if available.
	 */
	address?: string;
	/**
	 * When connected over HTTPS, this is the DER-encoded leaf certificate of the peer.
	 */
	certificate?: Buffer;
}

export class Response {
	/**
	 * The `bodyUsed` read-only property of the `Response` interface is a boolean value that indicates
	 * whether the body has been read yet.
	 *
	 * In Fáith, this indicates whether the body stream has ever been read from or canceled, as defined
	 * [in the spec](https://streams.spec.whatwg.org/#is-readable-stream-disturbed). Note that accessing
	 * the `.body` property counts as a read, even if you don't actually consume any bytes of content.
	 */
	readonly bodyUsed: boolean;
	/**
	 * The `headers` read-only property of the `Response` interface contains the `Headers` object
	 * associated with the response.
	 *
	 * Note that Fáith does not provide a custom `Headers` class; instead the Web API `Headers` structure
	 * is used directly and constructed by Fáith when needed.
	 */
	readonly headers: Headers;
	/**
	 * The `ok` read-only property of the `Response` interface contains a boolean stating whether the
	 * response was successful (status in the range 200-299) or not.
	 */
	readonly ok: boolean;
	/**
	 * Custom to Fáith.
	 *
	 * The `peer` read-only property of the `Response` interface contains an object with information about
	 * the remote peer that sent this response:
	 */
	readonly peer: PeerInformation;
	/**
	 * The `redirected` read-only property of the `Response` interface indicates whether or not the
	 * response is the result of a request you made which was redirected.
	 *
	 * Note that by the time you read this property, the redirect will already have happened, and you
	 * cannot prevent it by aborting the fetch at this point.
	 */
	readonly redirected: boolean;
	/**
	 * The `status` read-only property of the `Response` interface contains the HTTP status codes of the
	 * response. For example, 200 for success, 404 if the resource could not be found.
	 */
	readonly status: number;
	/**
	 * The `statusText` read-only property of the `Response` interface contains the status message
	 * corresponding to the HTTP status code in `Response.status`. For example, this would be `OK` for a
	 * status code `200`, `Continue` for `100`, `Not Found` for `404`.
	 *
	 * In HTTP/1, servers can send custom status text. This is returned here. In HTTP/2 and HTTP/3, custom
	 * status text is not supported at all, and the `statusText` property is either empty or simulated
	 * from well-known status codes.
	 */
	readonly statusText: string;
	/**
	 * The `type` read-only property of the `Response` interface contains the type of the response. The
	 * type determines whether scripts are able to access the response body and headers.
	 *
	 * In Fáith, this is always set to `basic`.
	 */
	readonly type: "basic";
	/**
	 * The `url` read-only property of the `Response` interface contains the URL of the response. The
	 * value of the `url` property will be the final URL obtained after any redirects.
	 */
	readonly url: string;
	/**
	 * The `version` read-only property of the `Response` interface contains the HTTP version of the
	 * response. The value will be the final HTTP version after any redirects and protocol upgrades.
	 *
	 * This is custom to Fáith.
	 */
	readonly version: string;

	/**
	 * The `body` read-only property of the `Response` interface is a `ReadableStream` of the body
	 * contents, or `null` for any actual HTTP response that has no body, such as `HEAD` requests and
	 * `204 No Content` responses.
	 *
	 * Note that browsers currently do not return `null` for those responses, but the spec requires
	 * it. Fáith chooses to respect the spec rather than the browsers in this case.
	 *
	 * An important consideration exists in conjunction with the connection pool: if you start the
	 * body stream, this will hold the connection until the stream is fully consumed. If another
	 * request is started during that time, and you don't have an available connection in the pool
	 * for the host already, the new request will open one.
	 */
	readonly body: ReadableStream<Uint8Array> | null;

	/**
	 * The `trailers()` read-only property of the `Response` interface returns a promise that
	 * resolves to either `null` or a `Headers` structure that contains the HTTP/2 or /3 trailing
	 * headers.
	 *
	 * This was once in the spec but was removed as it wasn't implemented by any browser.
	 *
	 * Note that this will never resolve if you don't also consume the body in some way.
	 */
	readonly trailers: Promise<Headers | null>;

	/**
	 * Discard the response body, releasing the connection back to the pool.
	 *
	 * This is useful when you don't need the body but want to ensure the connection
	 * can be reused for subsequent requests. If you don't call this and don't consume
	 * the body, the connection may be held open until the response is garbage collected.
	 *
	 * This is custom to Fáith.
	 *
	 * @returns {Promise<void>} Resolves when the body has been fully discarded
	 */
	discard(): Promise<void>;

	/**
	 * The `text()` method of the `Response` interface takes a `Response` stream and reads it to
	 * completion. It returns a promise that resolves with a `String`. The response is always decoded
	 * using UTF-8.
	 */
	text(): Promise<string>;

	/**
	 * The `bytes()` method of the `Response` interface takes a `Response` stream and reads it to
	 * completion. It returns a promise that resolves with a `Uint8Array`.
	 *
	 * In Fáith, this returns a Node.js `Buffer`, which can be used as (and is a subclass of) a `Uint8Array`.
	 */
	bytes(): Promise<Uint8Array>;

	/**
	 * The `arrayBuffer()` method of the `Response` interface takes a `Response` stream and reads it to
	 * completion. It returns a promise that resolves with an `ArrayBuffer`.
	 */
	arrayBuffer(): Promise<ArrayBuffer>;

	/**
	 * The `json()` method of the `Response` interface takes a `Response` stream and reads it to
	 * completion. It returns a promise which resolves with the result of parsing the body text as
	 * `JSON`.
	 *
	 * Note that despite the method being named `json()`, the result is not JSON but is instead the
	 * result of taking JSON as input and parsing it to produce a JavaScript object.
	 *
	 * Further note that, at least in Fáith, this method first reads the entire response body as bytes,
	 * and then parses that as JSON. This can use up to double the amount of memory. If you need more
	 * efficient access, consider handling the response body as a stream.
	 */
	json(): Promise<any>;

	/**
	 * The `blob()` method of the `Response` interface takes a `Response` stream and reads it to
	 * completion. It returns a promise that resolves with a `Blob`.
	 *
	 * The `type` of the `Blob` is set to the value of the `Content-Type` response header.
	 */
	blob(): Promise<Blob>;

	/**
	 * Fáith deliberately does not implement this. It will always throw.
	 */
	formData(): Promise<FormData>;

	/**
	 * The `clone()` method of the `Response` interface creates a clone of a response object, identical
	 * in every way, but stored in a different variable.
	 *
	 * `clone()` throws an `Error` if the response body has already been used.
	 */
	clone(): Response;

	/**
	 * This is entirely custom to Fáith. It returns a Web API `Response` instead of Fáith's custom
	 * `Response` class. However, it's not possible to construct a Web API `Response` that has all the
	 * properties of a Fáith Response (or of another Web Response, for that matter). So this method only
	 * returns a Response from:
	 *
	 * - the `body` stream
	 * - the `status`, `statusCode`, and `headers` properties
	 *
	 * Note that if `json()`, `bytes()`, etc has been called on the original response, the body stream
	 * of the new Web `Response` will be empty or inaccessible. If the body stream of the original
	 * response has been partially read, only the remaining bytes will be available in the new `Response`.
	 */
	webResponse(): globalThis.Response;
}

/**
 * Start fetching a resource from the network, returning a promise that is fulfilled once the
 * response is available.
 *
 * The promise resolves to the Response object representing the response to your request.
 *
 * A `fetch()` promise only rejects when the request fails, for example because of a badly-formed
 * request URL or a network error. A `fetch()` promise does not reject if the server responds with
 * HTTP status codes that indicate errors (404, 504, etc).
 */
export declare function fetch(
	/**
	 * This defines the resource that you wish to fetch. This can either be: (1) a string or any other
	 * object with a stringifier — including a `URL` object — that provides the URL of the resource
	 * you want to fetch. The URL must be absolute and include a scheme. Or (2) a `Request` object.
	 */
	resource: string | Request | URL | { toString(): string },
	/**
	 * A `RequestInit` object containing any custom settings that you want to apply to the request.
	 */
	options?: FetchOptions,
): Promise<Response>;
