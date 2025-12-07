# fáith - Rust-powered fetch API for Node.js

/ˈɸaːθj/ — pronounced FATH, like FATHER without the ER. This is an old irish word with the same
root as "fetch", meaning _poet_, _soothsayer_, _seer_, and later, _prophet_.

Fáith is of course a pun with _faith_, and is meant to be a _faithful_ implementation of the fetch
API for Node.js, but using a Rust-based network stack instead of undici + libuv.

Most `fetch` implementations for Node.js are based on the Node.js TCP stack (via libuv) and cannot
easily work around its limitations. The native fetch implementation, `undici`, explicitly targets
HTTP/1.1, and doesn't support HTTP/2+, among many other complaints.

Fáith tries to bring a Node.js fetch closer that is to the browser's fetch, notably by having
transparent support for HTTP/2 and HTTP/3, IPv6 and IPv4 using the "Happy Eyeballs" algorithm, an
HTTP cache and a cookie jar, a DNS cache, and actual support for `half` and `full` duplex modes.

## Installation

```bash
npm install faith
```

Or build from source:

```bash
git clone <repository>
cd faith
npm install
```

## Usage

### Basic fetch

```javascript
const { fetch } = require('faith');

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
const { fetch } = require('faith');
fetch(resource);
fetch(resource, options);
```

### Parameters

#### `resource`

*This defines the resource that you wish to fetch. This can either be:*

- *A string or any other object with a stringifier — including a `URL` object — that provides the
  URL of the resource you want to fetch.* The URL must be absolute and include a scheme.

- *A `Request` object.* This can be a native `Request` object, or a `faith.Request` object.

#### `options` (Optional)

*A `RequestInit` object containing any custom settings that you want to apply to the request.* In
practice the `RequestInit` class does not exist in browsers or Node.js, and so this is always a
"plain object" or "dictionary".

### Return value

*A `Promise` that resolves to a `Response` object.*

In `half` duplex mode (the default), the promise resolves when the request body has been fully sent
and the response headers have been received. In `full` duplex mode (supported by Fáith but not yet
browsers), the promise resolves as soon as response headers have been received, even if the request
body has not yet finished sending. Most HTTP servers will not send response headers until they've
finished receiving the body so this distinction doesn't matter, but some do, and it is possible to
take advantage of this behaviour with `full` duplex mode for decreased latency in specific cases.

### Exceptions

#### `AbortError` (DOMException)

*The request was aborted due to a call to the `AbortController.abort()` method.*

#### `NotAllowedError` (DOMException)

This is deliberately not implemented by Fáith.

#### `TypeError`

*Can occur for the following reasons:*

- *The requested URL is invalid.*
- *The requested URL includes credentials (username and password).*
- *The RequestInit object passed as the value of options included properties with invalid values.*
- *The request is blocked by a permissions policy.*
- *There is a network error (for example, because the device does not have connectivity).*

## `Request`

TBD

## `Response`

*The `Response` interface of the Fetch API represents the response to a request.*

Fáith does not allow its `Response` object to be constructed. If you need to, you may use the
`intoWebResponse()` method to convert one into a Web API `Response` object; note the caveats.

### `Response.body`

Fáith, due to technical restrictions, does not yet have `body` as a getter, but instead as a
method: `Response.body()`.

### `Response.bodyUsed`

*The `bodyUsed` read-only property of the `Response` interface is a boolean value that indicates
whether the body has been read yet.*

In Fáith, this indicates whether the body stream has ever been read from or canceled, as defined
[in the spec](https://streams.spec.whatwg.org/#is-readable-stream-disturbed).

This getter is "fused": once it returns `true`, it will always return `true`.

### `Response.headers`

*The `headers` read-only property of the `Response` interface contains the `Headers` object
associated with the response.*

### `Response.ok`

*The `ok` read-only property of the `Response` interface contains a boolean stating whether the
response was successful (status in the range 200-299) or not.*

### `Response.redirected`

*The `redirected` read-only property of the `Response` interface indicates whether or not the
response is the result of a request you made which was redirected.*

*Note that by the time you read this property, the redirect will already have happened, and you
cannot prevent it by aborting the fetch at this point.*

### `Response.status`

*The `status` read-only property of the `Response` interface contains the HTTP status codes of the
response. For example, 200 for success, 404 if the resource could not be found.*

*A value is `0` is returned for a response whose `type` is `opaque`, `opaqueredirect`, or `error`.*

### `Response.statusText`

*The `statusText` read-only property of the `Response` interface contains the status message
corresponding to the HTTP status code in `Response.status`. For example, this would be `OK` for a
status code `200`, `Continue` for `100`, `Not Found` for `404`.*

In HTTP/1, servers can send custom status text. This is returned here. In HTTP/2 and HTTP/3, custom
status text is not supported at all, and the `statusText` property is either empty or simulated
from well-known status codes.

### `Response.type`

Not yet implemented.

*The `type` read-only property of the `Response` interface contains the type of the response. The
type determines whether scripts are able to access the response body and headers.*

*It's a string, which may be any of the following cases:*

- *`basic`: the usual case.*
- `cors`: does not occur in Fáith.
- `error`: does not occur in Fáith.
- `opaque`: does not occur in Fáith.
- *`opaqueredirect`: A response to a request whose `redirect` option was set to `manual`, and which
  was redirected by the server. The `status` property is set to `0`, `body` is `null`, headers are
  empty and immutable.*

### `Response.url`

*The `url` read-only property of the `Response` interface contains the URL of the response. The
value of the `url` property will be the final URL obtained after any redirects.*

### `Response.arrayBuffer()`

Not yet implemented.

### `Response.blob()`

Not yet implemented.

### `Response.body()`

*The `body`* method *of the `Response` interface is a `ReadableStream` of the body
contents,* or `null` for any actual HTTP response that has no body, such as `HEAD` requests and
`204 No Content` responses.

Note that browsers currently do not return `null` for those responses, but the spec requires it.
Fáith chooses to respect the spec rather than the browsers in this case.

### `Response.bytes()`

*The `bytes()` method of the `Response` interface takes a `Response` stream and reads it to
completion. It returns a promise that resolves with a `Uint8Array`.*

### `Response.clone()`

Not yet implemented.

### `Response.formData()`

Not yet implemented.

### `Response.json()`

Not yet implemented.

### `Response.text()`

*The `text()` method of the `Response` interface takes a `Response` stream and reads it to
completion. It returns a promise that resolves with a `String`. The response is always decoded
using UTF-8.*
