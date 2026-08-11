/**
 * The capability vocabulary.
 *
 * Servers declare what they can do; dimensions declare what they need. The
 * runner intersects the two. Keeping the names in one frozen object means a
 * typo fails loudly rather than silently skipping every cell that mentions it —
 * which would look exactly like a passing run with nothing to do.
 */

const CAPABILITIES = Object.freeze({
	// protocol versions the server will negotiate
	H1: "h1",
	H2: "h2",
	H3: "h3",
	// advertises HTTP/3 via Alt-Svc on a TCP response
	ALTSVC: "altsvc",
	// offers both http/1.1 and h2 in ALPN, so the client's preference is what
	// decides. A single-protocol row cannot test a preference.
	ALPN_MULTI: "alpnMulti",
	// can emit response trailers
	TRAILERS: "trailers",
	// content codings it will apply
	GZIP: "gzip",
	BROTLI: "brotli",
	ZSTD: "zstd",
	// response framing it can produce. HTTP/2 has no chunked encoding, so an
	// h2-only server must not claim CHUNKED.
	CHUNKED: "chunked",
	CONTENT_LENGTH: "contentLength",
	// honours If-None-Match / If-Modified-Since with a 304
	CONDITIONAL: "conditional",
	// rejects oversized request headers with a 4xx
	HEADER_LIMITS: "headerLimits",
	// closes the connection after a configured number of requests
	KEEPALIVE_LIMIT: "keepaliveLimit",
	// drops a pooled connection straight after responding on it, with no in-band
	// close signal. Distinct from KEEPALIVE_LIMIT, where the closing response
	// carries `Connection: close` and the client is told: here it is told nothing,
	// so the client discovers the connection is gone only when it reuses it.
	IDLE_CLOSE: "idleClose",
	// can be made to send an HTTP/2 GOAWAY
	GOAWAY: "goaway",
	// TLS, and client-certificate authentication
	TLS: "tls",
	CLIENT_CERTS: "clientCerts",
	// can be instructed to misbehave, so a dimension's negative case can run
	SCRIPTABLE: "scriptable",
});

const KNOWN = new Set(Object.values(CAPABILITIES));

/** Throw if any name is outside the vocabulary. `context` names the culprit. */
function assertKnownCapabilities(names, context) {
	for (const name of names) {
		if (!KNOWN.has(name)) {
			throw new Error(
				`${context} declares unknown capability ${JSON.stringify(name)}; ` +
					`known capabilities are ${[...KNOWN].sort().join(", ")}`,
			);
		}
	}
}

module.exports = { CAPABILITIES, assertKnownCapabilities };
