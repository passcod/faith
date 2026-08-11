# Method normalisation

Scenarios verifying that only the six methods the fetch standard normalises are upper-cased, and that every other method reaches the server with the case the caller wrote.

The echoing origin in `test/http-methods.test.js` is a raw TCP listener, because llhttp only parses methods from its own table and rejects a custom method before any handler sees it.

## Normalised methods

- [x] `delete`, `get`, `head`, `options`, `post`, `put` each reach the server upper-cased (verifies spec: REQ#method-and-headers)
- [x] Mixed case (`GeT`, `Put`, `hEaD`) normalises the same way, matching case-insensitively byte by byte (verifies spec: REQ#method-and-headers)
- [x] A lowercase `head` is still treated as a HEAD request end to end: the response carries no body (verifies spec: REQ#method-and-headers)

## Methods passed through

- [x] `patch` stays lower case, since PATCH is outside the set the standard normalises (verifies spec: REQ#method-and-headers)
- [x] A custom method (`Frobnicate`) reaches the server with its case intact, so a server routing case-sensitively sees what the caller wrote (verifies spec: REQ#method-and-headers)
- [x] `m-SEARCH` and `purge` reach the server unchanged (verifies spec: REQ#method-and-headers)

## Invalid methods

- [x] A method with characters outside the HTTP token set throws an invalid-method error (verifies spec: REQ#method-and-headers)
- [x] `OPTıONS` throws an invalid-method error rather than being normalised: uppercasing it the Unicode way would produce `OPTIONS` and smuggle non-token bytes past validation (verifies spec: REQ#method-and-headers)

## Interactions

- [x] A 307 redirect replays a custom method with its case intact (verifies spec: REDIR#method-and-body-while-following)
- [x] A custom method reaches the server unchanged through an agent with a cache configured (verifies spec: CACHE#what-a-stored-response-answers)
