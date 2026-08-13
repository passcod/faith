# Full duplex mode test cases

Faith runs full duplex and does not act on the `duplex` value.
The behaviour holds because of how reqwest and hyper compose rather than because Faith asks for it, so the sequencing cases matter more than usual: a stack upgrade could take the capability away without any other test noticing.

## Duplex sequencing

Each case runs over both HTTP/1.1 and HTTP/2, since the two reach full duplex by different routes and a regression could take one without the other. Each asserts the negotiated version so a silent downgrade cannot pass.

- [x] The response is surfaced while the request body is still open (verifies spec: REQ)
- [x] The response body can be read while the request body is still open (verifies spec: REQ)
- [x] The request body can be driven from what is read off the response body, over one request (verifies spec: REQ)
- [ ] A buffered (non-stream) body also surfaces its response before the body has finished being written

## The duplex option

- [x] A `ReadableStream` body without `duplex` throws a `TypeError` naming the option (verifies spec: REQ)
- [x] A `ReadableStream` body with `duplex: "half"` is accepted and sends the body (verifies spec: REQ)
- [x] String, `Buffer`, `Uint8Array` and `ArrayBuffer` bodies need no `duplex` (verifies spec: REQ)
- [ ] `duplex: "half"` does not change sequencing: the same request runs full duplex with it set and with a buffered body that omits it (verifies spec: REQ)

## Interactions

- [ ] A request whose origin answers and then stops reading the body surfaces its response rather than hanging
- [ ] `signal` aborts a request whose body is still streaming and whose response has already been surfaced
- [ ] `timeout` ends a request whose origin neither reads the body nor answers (verifies spec: CANCEL)

Notes: the timeout case was checked by hand while the spike was being unwound and behaves correctly, rejecting with `Timeout` at +4009ms against a 4s deadline. It is left unticked because no automated test covers it.
