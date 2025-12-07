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

## API Reference

Conforms to the [fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API).

In the following documentation, italics are parts that are *identical to how native fetch works*
(as per MDN), and non-italics document where behaviour varies and is specific to fáith (unless
otherwise specified).

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
