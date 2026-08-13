# Full duplex mode test cases

Faith runs full duplex and does not act on the `duplex` value.
The behaviour holds because of how reqwest and hyper compose rather than because Faith asks for it, so the sequencing cases matter more than usual: a stack upgrade could take the capability away without any other test noticing.

## Duplex sequencing

- [x] The response is surfaced while the request body is still open, over HTTP/1.1 (verifies spec: REQ)
- [x] The response body can be read while the request body is still open (verifies spec: REQ)
- [x] The request body can be driven from what is read off the response body, over one request (verifies spec: REQ)
- [ ] The same three cases over HTTP/2, where the transport multiplexes rather than relying on the HTTP/1.1 divergence
- [ ] A buffered (non-stream) body also surfaces its response before the body has finished being written

## The duplex option

- [x] A `ReadableStream` body without `duplex` throws a `TypeError` naming the option (verifies spec: REQ)
- [x] A `ReadableStream` body with `duplex: "half"` is accepted and sends the body (verifies spec: REQ)
- [x] String, `Buffer`, `Uint8Array` and `ArrayBuffer` bodies need no `duplex` (verifies spec: REQ)
- [ ] `duplex: "half"` does not change sequencing: the same request runs full duplex with it set and with a buffered body that omits it (verifies spec: REQ)

## Interactions

- [ ] A request whose origin answers and then stops reading the body fails rather than hanging
- [ ] `signal` aborts a request whose body is still streaming and whose response has already been surfaced
- [ ] `timeout` applies to a request whose body is still streaming after the response was surfaced

Notes: the last item is the one to watch. A stalled origin was measured taking 30.5s to fail with a 4s `timeout` set, because the per-request timeout is handed to reqwest and stops applying once the response head is in. That is present behaviour rather than something this card introduced, and it wants confirming as a separate concern.
