# fáith - Rust-powered fetch API for Node.js

/ˈɸaːθj/ — pronounced FATH, like FATHER without the ER. This is an old irish word with the same
root as "fetch", meaning _poet_, _soothsayer_, _seer_, and later, _prophet_.

Fáith is of course a pun with _faith_, and is meant to be a _faithful_ implementation of the fetch
API for Node.js, but using a Rust-based network stack instead of undici + libuv.

Most `fetch` implementations for Node.js are based on the Node.js TCP stack (via libuv) and cannot
easily work around its limitations. The native fetch implementation, `undici`, explicitly targets
HTTP/1.1, and doesn't support HTTP/2+, among many other complaints.

Fáith tries to bring a Node.js fetch that is closer to the browser's fetch, notably by having
transparent support for HTTP/2 and HTTP/3, IPv6 and IPv4 using the "Happy Eyeballs" algorithm, an
HTTP cache and a cookie jar, a DNS cache, and actual support for `half` and `full` duplex modes.

## Installation

```bash
npm install @passcod/faith
```

## Usage

### Basic fetch

```javascript
import { fetch } from '@passcod/faith';

async function example() {
  const response = await fetch('https://httpbin.org/get');
  console.log(response.status); // 200
  console.log(response.ok); // true

  const data = response.json();
  console.log(data.url); // https://httpbin.org/get
}
```

### Fetch with options

```javascript
const response = await fetch('https://httpbin.org/post', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'X-Custom-Header': 'value'
  },
  body: JSON.stringify({ message: 'Hello' }),
  timeout: 30 // seconds
});
```

# API Reference

Conforms to the [fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API).

In the following documentation, italics are parts that are *identical to how native fetch works*
(as per MDN), and non-italics document where behaviour varies and is specific to fáith (unless
otherwise specified).

## `fetch()`

### Syntax

```javascript
import { fetch } from '@passcod/faith';
fetch(resource);
fetch(resource, options);
```

### Parameters

#### `resource`

*This defines the resource that you wish to fetch. This can either be:*

- *A string or any other object with a stringifier — including a `URL` object — that provides the
  URL of the resource you want to fetch.* The URL must be absolute and include a scheme.

- *A `Request` object.*

#### `options` (Optional)

*A `RequestInit` object containing any custom settings that you want to apply to the request.* In
practice the `RequestInit` class does not exist in browsers or Node.js, and so this is always a
"plain object" or "dictionary". The fields supported by Fáith are documented below.

### Return value

*A `Promise` that resolves to a `Response` object.*

In `half` duplex mode (the default), the promise resolves when the request body has been fully sent
and the response headers have been received. In `full` duplex mode (supported by Fáith but not yet
browsers), the promise resolves as soon as response headers have been received, even if the request
body has not yet finished sending. Most HTTP servers will not send response headers until they've
finished receiving the body so this distinction doesn't matter, but some do, and it is possible to
take advantage of this behaviour with `full` duplex mode for decreased latency in specific cases.
You may even be able to vary the request body stream based on the response body stream.

## `Request`

Fáith does not implement its own `Request` object. Instead, you can pass a Web API `Request` object
to `fetch()`, and it will internally be converted to the right options.

## `RequestInit` object

*The `RequestInit` dictionary of the Fetch API represents the set of options that can be used to
configure a fetch request.*

*You can pass a `RequestInit` object into the `Request()` constructor, or directly into the
`fetch()` function call.* Note that Fáith has additional options available, and those will not
survive a trip through `Request`. Prefer to supply `RequestInit` directly to `fetch()`.

*You can also construct a `Request` with a `RequestInit`, and pass the `Request` to a `fetch()`
call along with another `RequestInit`. If you do this, and the same option is set in both places,
then the value passed directly into `fetch()` is used.*

Note that you can include options that Fáith does not support; they will simply be ignored.

### `FetchOptions.agent: Agent`

This is custom to Fáith.

You can create an `Agent`, and pass it here to have the request executed by the `Agent`. See the
documentation for the `Agent` options you can set with this, and the agent data you can access.
Notably an agent has a DNS cache, and may be configured to handle cookies and/or an HTTP cache.

When not provided, a global default `Agent` is created on first use.

### `FetchOptions.attributionReporting`

Fáith deliberately does not implement this.

### `FetchOptions.body`

*The request body contains content to send to the server, for example in a `POST` or `PUT` request.
It is specified as an instance of any of the following types:*

- *a string*
- *`ArrayBuffer`*
- *`Blob`*
- *`DataView`*
- *`File`*
- *`FormData`*
- *`TypedArray`*
- *`URLSearchParams`* Not yet implemented.
- *`ReadableStream`* Note that Fáith currently reads this into memory before sending the request.

*If `body` is a `ReadableStream`, the `duplex` option must also be set.*

### `FetchOptions.browsingTopics`

Fáith deliberately does not implement this.

### `FetchOptions.cache`

Fáith does not respect this option on the `RequestInit` dictionary. Instead, the option is present
on `Agent` and applies to all requests made with that `Agent`.

### `FetchOptions.credentials: string`

*Controls whether or not the browser sends credentials with the request, as well as whether any
`Set-Cookie` response headers are respected. Credentials are cookies, ~~TLS client certificates,~~
or authentication headers containing a username and password. This option may be any one of the
following values:*

- *`omit`: Never send credentials in the request or include credentials in the response.*
- ~~`same-origin`~~: Fáith does not implement this, as there is no concept of "origin" on the server.
- *`include`: *Always include credentials,* ~~even for cross-origin requests.~~

Fáith ignores the `Access-Control-Allow-Credentials` and `Access-Control-Allow-Origin` headers.

Fáith currently does not `omit` the TLS client certificate when the request's `Agent` has one
configured. This is an upstream limitation.

If the request's `Agent` has cookies enabled, new cookies from the response will be added to the
cookie jar, even as Fáith strips them from the request and response headers returned to the user.
This is an upstream limitation.

Defaults to `include` (browsers default to `same-origin`).

### `FetchOptions.duplex: string`

*Controls duplex behavior of the request. If this is present it must have the value `half`, meaning
that Fáith will send the entire request before processing the response.*

*This option must be present when `body` is a `ReadableStream`.*

### `FetchOptions.headers: Headers | object`

*Any headers you want to add to your request, contained within a `Headers` object or an object
literal whose keys are the names of headers and whose values are the header values.*

Fáith allows all request headers to be set (unlike browsers, which [forbid][1] a number of them).

[1]: https://developer.mozilla.org/en-US/docs/Glossary/Forbidden_request_header

### `FetchOptions.integrity: string`

Not implemented yet.

*Contains the subresource integrity value of the request.*

*The format of this option is `<hash-algo>-<hash-source>` where:*

- *`<hash-algo>` is one of the following values: `sha256`, `sha384`, or `sha512`*
- *`<hash-source>` is the Base64-encoding of the result of hashing the resource with the specified
  hash algorithm.*

Fáith only checks the integrity when using `bytes()`, `json()`, `text()`, `arrayBuffer()`, and
`blob()`. Verification when reading through the `body` stream is not currently supported.

Note that browsers will throw at the `fetch()` call when integrity fails, but Fáith will only throw
when the above methods are called, as until then the body contents are not available.

### `FetchOptions.keepalive`

Not supported.

Note that this is different from `Connection: keep-alive`; Fáith connections are pooled within each
single `Agent`, so subsequent requests to the same endpoint are faster until the pooled connection
times out. The `keepalive` option in browsers is instead a way to send a `fetch()` right before the
page is unloaded, for tracking or analytics purposes. This concept does not exist in Node.js.

### `FetchOptions.method: string`

*The request method. Defaults to `GET`.*

### `FetchOptions.mode`

Fáith deliberately does not implement this, as there is no CORS/origin.

### `FetchOptions.priority`

Not supported.

### `FetchOptions.redirect`

Fáith does not respect this option on the `RequestInit` dictionary. Instead, the option is present
on `Agent` and applies to all requests made with that `Agent`.

### `FetchOptions.referrer`

Fáith deliberately does not implement this, as there is no origin.

However, Fáith does set the `Referer` header when redirecting automatically.

### `FetchOptions.referrerPolicy`

Fáith deliberately does not implement this, as there is no origin.

However, Fáith does set the `Referer` header when redirecting automatically.

### `FetchOptions.signal: AbortSignal`

*An `AbortSignal`. If this option is set, the request can be canceled by calling `abort()` on the
corresponding `AbortController`.*

### `FetchOptions.timeout: number`

Custom to Fáith. Cancels the request after this many milliseconds.

This will give a different error to using `signal` with a timeout, which might be preferable in
some cases. It also has a slightly different internal behaviour: `signal` may abort the request
only until the response headers have been received, while `timeout` will apply through the entire
response receipt.

## `Response`

*The `Response` interface of the Fetch API represents the response to a request.*

Fáith does not allow its `Response` object to be constructed. If you need to, you may use the
`webResponse()` method to convert one into a Web API `Response` object; note the caveats.

### `Response.body: ReadableStream | null`

*The `body` read-only property of the `Response` interface is a `ReadableStream` of the body
contents,* or `null` for any actual HTTP response that has no body, such as `HEAD` requests and
`204 No Content` responses.

Note that browsers currently do not return `null` for those responses, but the spec requires it.
Fáith chooses to respect the spec rather than the browsers in this case.

### `Response.bodyUsed: boolean`

*The `bodyUsed` read-only property of the `Response` interface is a boolean value that indicates
whether the body has been read yet.*

In Fáith, this indicates whether the body stream has ever been read from or canceled, as defined
[in the spec](https://streams.spec.whatwg.org/#is-readable-stream-disturbed). Note that accessing
the `.body` property counts as a read, even if you don't actually consume any bytes of content.

### `Response.headers: Headers`

*The `headers` read-only property of the `Response` interface contains the `Headers` object
associated with the response.*

Note that Fáith does not provide a custom `Headers` class; instead the Web API `Headers` structure
is used directly and constructed by Fáith when needed.

### `Response.ok: boolean`

*The `ok` read-only property of the `Response` interface contains a boolean stating whether the
response was successful (status in the range 200-299) or not.*

### `Response.peer: object`

Custom to Fáith.

The `peer` read-only property of the `Response` interface contains an object with information about
the remote peer that sent this response:

#### `Response.peer.address: string | null`

The IP address and port of the peer, if available.

#### `Response.peer.certificate: Buffer | null`

When connected over HTTPS, this is the DER-encoded leaf certificate of the peer.

### `Response.redirected: boolean`

*The `redirected` read-only property of the `Response` interface indicates whether or not the
response is the result of a request you made which was redirected.*

*Note that by the time you read this property, the redirect will already have happened, and you
cannot prevent it by aborting the fetch at this point.*

### `Response.status: number`

*The `status` read-only property of the `Response` interface contains the HTTP status codes of the
response. For example, 200 for success, 404 if the resource could not be found.*

*A value is `0` is returned for a response whose `type` is `opaque`, `opaqueredirect`, or `error`.*

### `Response.statusText: string`

*The `statusText` read-only property of the `Response` interface contains the status message
corresponding to the HTTP status code in `Response.status`. For example, this would be `OK` for a
status code `200`, `Continue` for `100`, `Not Found` for `404`.*

In HTTP/1, servers can send custom status text. This is returned here. In HTTP/2 and HTTP/3, custom
status text is not supported at all, and the `statusText` property is either empty or simulated
from well-known status codes.

### `Response.type: string`

*The `type` read-only property of the `Response` interface contains the type of the response. The
type determines whether scripts are able to access the response body and headers.*

In Fáith, this is always set to `basic`.

### `Response.url: string`

*The `url` read-only property of the `Response` interface contains the URL of the response. The
value of the `url` property will be the final URL obtained after any redirects.*

### `Response.version: string`

The `version` read-only property of the `Response` interface contains the HTTP version of the
response. The value will be the final HTTP version after any redirects and protocol upgrades.

This is custom to Fáith.

### `Response.arrayBuffer(): Promise<ArrayBuffer>`

*The `arrayBuffer()` method of the `Response` interface takes a `Response` stream and reads it to
completion. It returns a promise that resolves with an `ArrayBuffer`.*

### `Response.blob(): Promise<Blob>`

*The `blob()` method of the `Response` interface takes a `Response` stream and reads it to
completion. It returns a promise that resolves with a `Blob`.*

*The `type` of the `Blob` is set to the value of the `Content-Type` response header.*

### `Response.bytes(): Promise<Buffer>`

*The `bytes()` method of the `Response` interface takes a `Response` stream and reads it to
completion. It returns a promise that resolves with a `Uint8Array`.*

In Fáith, this returns a Node.js `Buffer`, which can be used as (and is a subclass of) a `Uint8Array`.

### `Response.clone(): Response`

*The `clone()` method of the `Response` interface creates a clone of a response object, identical
in every way, but stored in a different variable.*

*`clone()` throws* an `Error` *if the response body has already been used.*

(In-spec, this should throw a `TypeError`, but for technical reasons this is not possible with Fáith.)

### `Response.formData(): !`

Fáith deliberately does not implement this. The method exists so the types work out, but it will
always throw.

### `Response.json(): Promise<unknown>`

*The `json()` method of the `Response` interface takes a `Response` stream and reads it to
completion. It returns a promise which resolves with the result of parsing the body text as
`JSON`.*

*Note that despite the method being named `json()`, the result is not JSON but is instead the
result of taking JSON as input and parsing it to produce a JavaScript object.*

Further note that, at least in Fáith, this method first reads the entire response body as bytes,
and then parses that as JSON. This can use up to double the amount of memory. If you need more
efficient access, consider handling the response body as a stream.

### `Response.text(): Promise<string>`

*The `text()` method of the `Response` interface takes a `Response` stream and reads it to
completion. It returns a promise that resolves with a `String`. The response is always decoded
using UTF-8.*

### `Response.webResponse(): globalThis.Response`

This is entirely custom to Fáith. It returns a Web API `Response` instead of Fáith's custom
`Response` class. However, it's not possible to construct a Web API `Response` that has all the
properties of a Fáith Response (or of another Web Response, for that matter). So this method only
returns a Response from:

- the `body` stream
- the `status`, `statusCode`, and `headers` properties

Note that if `json()`, `bytes()`, etc has been called on the original response, the body stream
of the new Web `Response` will be empty or inaccessible. If the body stream of the original
response has been partially read, only the remaining bytes will be available in the new `Response`.

## `Agent`

The `Agent` interface of the Fáith API represents an instance of an HTTP client. Each `Agent` has
its own options, connection pool, caches, etc. There are also conveniences such as `headers` for
setting default headers on all requests done with the agent, and statistics collected by the agent.

Re-using connections between requests is a significant performance improvement: not only because
the TCP and TLS handshake is only performed once across many different requests, but also because
the DNS lookup doesn't need to occur for subsequent requests on the same connection. Depending on
DNS technology (DoH and DoT add a whole separate handshake to the process) and overall latency,
this can not only speed up requests on average, but also reduce system load.

For this reason, and also because in browsers this behaviour is standard, **all** requests with
Fáith use an `Agent`. For `fetch()` calls that don't specify one explicitly, a global agent with
default options is created on first use.

### Syntax

```javascript
new Agent()
new Agent(options)
```

### `AgentOptions.cache`
### `AgentOptions.cookies: bool`

Enable a persistent cookie store for the agent. Cookies received in responses will be preserved and
included in additional requests.

Default: `false`.

You may use `agent.getCookie(url: string)` and `agent.addCookie(url: string, value: string)` to add
and retrieve cookies from the store.

### `AgentOptions.dns`
### `AgentOptions.headers: Array<{ name: string, value: string, sensitive?: bool }>`

Sets the default headers for every request.

If header names or values are invalid, they are silently omitted.
Sensitive headers (e.g. `Authorization`) should be marked.

Default: none.

### `AgentOptions.http3: object`

Settings related to HTTP/3. This is a nested object.

#### `AgentOptions.http3.congestion: string`

The congestion control algorithm. The default is `cubic`, which is the same used in TCP in the
Linux stack. It's fair for all traffic, but not the most optimal, especially for networks with
a lot of available bandwidth, high latency, or a lot of packet loss. Cubic reacts to packet loss by
dropping the speed by 30%, and takes a long time to recover. BBR instead tries to maximise
bandwidth use and optimises for round-trip time, while ignoring packet loss.

In some networks, BBR can lead to pathological degradation of overall network conditions, by
flooding the network by up to **100 times** more retransmissions. This is fixed in BBRv2 and BBRv3,
but Fáith (or rather its underlying QUIC library quinn, [does not implement those yet][2]).

[2]: https://github.com/quinn-rs/quinn/issues/1254

Default: `cubic`. Accepted values: `cubic`, `bbr1`.

#### `AgentOptions.http3.maxIdleTimeout: number`

Maximum duration of inactivity to accept before timing out the connection, in seconds. Note that
this only sets the timeout on this side of the connection: the true idle timeout is the _minimum_
of this and the peer’s own max idle timeout. While the underlying library has no limits, Fáith
defines bounds for safety: minimum 1 second, maximum 2 minutes (120 seconds).

Default: 30.

### `AgentOptions.pool: object`

Settings related to the connection pool. This is a nested object.

#### `AgentOptions.pool.idleTimeout: number`

How many seconds of inactivity before a connection is closed.

Default: 90 seconds.

#### `AgentOptions.pool.maxIdlePerHost: number | null`

The maximum amount of idle connections per host to allow in the pool. Connections will be closed
to keep the idle connections (per host) under that number.

Default: `null` (no limit).

### `AgentOptions.redirect`
### `AgentOptions.retry`
### `AgentOptions.timeout`
#### `AgentOptions.timeout.connect`
#### `AgentOptions.timeout.read`
#### `AgentOptions.timeout.total`
### `AgentOptions.tls`
#### `AgentOptions.tls.earlyData`
#### `AgentOptions.tls.identity`
#### `AgentOptions.tls.required`

### `AgentOptions.userAgent`

Custom user agent string.

Default: `Faith/{version} reqwest/{version}`.

You may use the `USER_AGENT` constant if you wish to prepend your own agent to the default, e.g.

```javascript
import { Agent, USER_AGENT } from '@passcod/faith';
const agent = new Agent({
  userAgent: `YourApp/1.2.3 ${USER_AGENT}`,
});
```

### `Agent.addCookie(url: string, cookie: string)`

Add a cookie into the agent.

Does nothing if:
- the cookie store is disabled
- the url is malformed

### `Agent.getCookie(url: string): string | null`

Retrieve a cookie from the store.

Returns `null` if:
- there's no cookie at this url
- the cookie store is disabled
- the url is malformed
- the cookie cannot be represented as a string

### `Agent.stats(): object`

Returns statistics gathered by this agent:

- `requestsSent`
- `responsesReceived`

## Error mapping

Fáith produces fine-grained errors, but maps them to a few javascript error types for fetch
compatibility. The `.code` property on errors thrown from Fáith is set to a stable name for each
error kind, documented in this comprehensive mapping:

- JS `AbortError`:
  - `Aborted` — request was aborted using `signal`
  - `Timeout` — request timed out
- JS `NetworkError`:
  - `Network` — network error
- JS `SyntaxError`:
  - `JsonParse` — JSON parse error for `response.json()`
  - `Utf8Parse` — UTF8 decoding error for `response.text()`
- JS `TypeError`:
  - `InvalidHeader` — invalid header name or value
  - `InvalidMethod` — invalid HTTP method
  - `InvalidUrl` — invalid URL string
  - `ResponseAlreadyDisturbed` — body already read (mutually exclusive operations)
  - `ResponseBodyNotAvailable` — body is null or not available
- JS generic `Error`:
  - `BodyStream` — internal stream handling error
  - `RuntimeThread` — failed to start or schedule threads on the internal tokio runtime

The library exports an `ERROR_CODES` object which has every error code the library throws, and
every error thrown also has a `code` property that is set to one of those codes. So you can
accurately respond to the exact error kind by checking its code and matching against the right
constant from `ERROR_CODES`, instead of doing string matching on the error message, or coarse
`instance of` matching.

Due to technical limitations, when reading a body stream, reads might fail, but that error
will not have a `.code` property.
