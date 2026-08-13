# X2: Disallow streaming request bodies over HTTP/1.1 by default

## How the rule is enforced

The fetch standard's check runs after a connection is chosen and before the body is sent, which is exactly where `hyper-util`'s legacy client already checks: given a request whose version is `HTTP_2`, it rejects with `UserUnsupportedVersion` once it has a pooled connection that turns out to be HTTP/1. So setting the request version to `HTTP_2` on a streaming body delegates the check to the layer that knows the answer, and no body bytes go out.

Faith never enables h2c prior knowledge, so a plaintext `http://` connection is always HTTP/1.x. That case is known before connecting and is refused up front, which avoids opening a connection to fail on and gives the caller a message that names the rule. A `https://` origin's protocol is only known after ALPN, so that case rides the hyper check.

The HTTP/3 path needs no change: the upgrade attempt is already skipped for a body that cannot be replayed (`try_clone()` returns `None` for a stream), per H3UP.

## Steps

- [x] Add the `quirks` agent option group with `h1RequestStreaming`, resolved onto the agent
- [x] Refuse a streaming body on a plaintext origin up front when the quirk is off
- [x] Assert `HTTP_2` on a streaming body over TLS when the quirk is off
- [x] Point the existing streaming tests that target the plain-HTTP/1.1 httpbin at an agent with the quirk on
- [x] Add tests for the rule itself, both directions, and for the error's code
  - [x] Plaintext, both directions, asserting the origin saw nothing on a refusal
  - [x] TLS pinned to one ALPN protocol, covering both the HTTP/1.1 refusal and the HTTP/2 success
- [x] Document the option in the README
- [x] Release the stream on a refusal, so the chunk pump cannot strand and hold the process open
- [x] Point the full-duplex HTTP/1.1 sequencing tests at the quirk, and say so in the duplex docs
- [x] Destroy accepted sockets when closing a test origin, so a pooled connection cannot hold the run open on Node 20 and 22

## Notes

The existing streaming tests all target the httpbin origin over plain HTTP/1.1, so they exercise the very thing now refused. They keep their coverage of the streaming machinery by opting into the quirk, and the rule gets its own tests.

Refusing has to take the receiving end of the chunk channel and drop it. The JS side pumps chunks into that channel from the caller's stream as soon as the request starts, so a refusal that left the receiver in place stranded the pump: it filled the channel, blocked on the next push, and held the process open with no way out. That is what turned the failure into a hang rather than a test failure, and it is covered by a test that runs a refusal in a child process and asserts the child exits.

Full duplex over HTTP/1.1 rides on a streaming request body, so it now needs the quirk. That is the trade the card asks for, and the duplex documentation says so rather than leaving the promise unqualified.

A test origin has to destroy the sockets it accepted, not just stop listening: a pooled keep-alive connection keeps an accepted socket open, and on Node 20 and 22 that socket holds the event loop after the assertions have all passed, so the run finishes its output and then sits there. Node 24 and 26 tidy it up on their own, which is why the same suite passed locally and hung on half the CI matrix. `duplex-sequencing.test.js` had already met this and carried its own helper; that helper now lives in `test/fixtures/net.js` as `trackSockets` and both files use it.
