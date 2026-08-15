/**
 * A controllable authoritative DNS server, for testing and benchmarking Faith's
 * resolver.
 *
 * `dns.overrides` resolves a name without ever asking a nameserver, so it can't
 * exercise the resolver's cache at all: no lookup happens, nothing is cached,
 * and no TTL expires. Anything about cache behaviour — a TTL lapsing, a stale
 * answer being served, a background refresh picking up a changed address —
 * needs a real nameserver that answers on demand, with answers the test
 * controls and a query log the test can assert against.
 *
 * The knobs exist for what those tests need to observe:
 *
 *   - `ttl` decides when a cached answer goes stale, so tests set it low
 *     (1 second) rather than waiting out a realistic TTL
 *   - `delayMs` makes the resolver slow, which is the only condition under
 *     which serving stale beats resolving fresh; a fast local answer hides the
 *     very difference a bench row is meant to show
 *   - the query log distinguishes "served from cache without asking" from
 *     "asked the nameserver", which is otherwise invisible from the outside
 *   - `set()` changes an address mid-run, so a test can tell a background
 *     refresh actually landed rather than assuming it did
 *   - `fail()` covers a refresh that errors or hangs, where the question is
 *     whether the previous answer survives
 *
 * Both UDP and TCP are served on the same port. Hickory falls back to TCP on a
 * truncated or failed UDP exchange, and a server that only spoke UDP would turn
 * that fallback into a hang.
 *
 * Negative answers carry an authority-section SOA, without which a conforming
 * resolver declines to cache them at all (RFC 2308). `negativeTtl` sets how long
 * they may be cached for.
 *
 * Not a general-purpose nameserver: one question per message, A and AAAA only,
 * no CNAME chains, no delegation, no DNSSEC. The SOA it sends is owned by the
 * queried name rather than a zone apex, which is enough for a resolver to take a
 * negative TTL from but is not how a real zone is laid out.
 */

const dgram = require("node:dgram");
const net = require("node:net");

const TYPE_A = 1;
const TYPE_SOA = 6;
const TYPE_AAAA = 28;
const TYPE_OPT = 41;
const TYPE_HTTPS = 65;
const CLASS_IN = 1;

/** SvcParamKeys this helper can encode (RFC 9460 §14.3.2). */
const SVCPARAM_ALPN = 1;
const SVCPARAM_PORT = 3;

const RCODE_NOERROR = 0;
const RCODE_FORMERR = 1;
const RCODE_SERVFAIL = 2;
const RCODE_NXDOMAIN = 3;
const RCODE_NOTIMP = 4;

const TYPE_NAMES = {
	[TYPE_A]: "A",
	[TYPE_AAAA]: "AAAA",
	[TYPE_HTTPS]: "HTTPS",
};

/** Lowercase and drop the root dot, so zone lookups match however a client asks. */
function normaliseName(name) {
	return name.replace(/\.$/, "").toLowerCase();
}

function ipv4ToBuffer(str) {
	const parts = str.split(".");
	if (parts.length !== 4) throw new Error(`not an IPv4 address: ${str}`);
	const buf = Buffer.alloc(4);
	for (let i = 0; i < 4; i++) {
		const n = Number(parts[i]);
		if (!Number.isInteger(n) || n < 0 || n > 255) {
			throw new Error(`not an IPv4 address: ${str}`);
		}
		buf[i] = n;
	}
	return buf;
}

function ipv6ToBuffer(str) {
	// Reject the IPv4-mapped forms rather than half-supporting them; the zone
	// only ever needs plain v6 literals.
	if (str.includes(".")) throw new Error(`unsupported IPv6 form: ${str}`);
	const halves = str.split("::");
	if (halves.length > 2) throw new Error(`not an IPv6 address: ${str}`);
	const head = halves[0] ? halves[0].split(":") : [];
	const tail = halves.length === 2 && halves[1] ? halves[1].split(":") : [];
	let groups;
	if (halves.length === 1) {
		groups = head;
	} else {
		const gap = 8 - head.length - tail.length;
		if (gap < 1) throw new Error(`not an IPv6 address: ${str}`);
		groups = [...head, ...Array(gap).fill("0"), ...tail];
	}
	if (groups.length !== 8) throw new Error(`not an IPv6 address: ${str}`);
	const buf = Buffer.alloc(16);
	for (let i = 0; i < 8; i++) {
		if (!/^[0-9a-fA-F]{1,4}$/.test(groups[i])) {
			throw new Error(`not an IPv6 address: ${str}`);
		}
		buf.writeUInt16BE(Number.parseInt(groups[i], 16), i * 2);
	}
	return buf;
}

function addressToRdata(address, type) {
	return type === TYPE_AAAA ? ipv6ToBuffer(address) : ipv4ToBuffer(address);
}

function encodeName(name) {
	const labels = name
		.split(".")
		.filter(Boolean)
		.map((label) => {
			const buf = Buffer.alloc(1 + label.length);
			buf.writeUInt8(label.length, 0);
			buf.write(label, 1, "latin1");
			return buf;
		});
	return Buffer.concat([...labels, Buffer.from([0])]);
}

/**
 * HTTPS (SVCB) RDATA: `SvcPriority TargetName SvcParams` (RFC 9460 §2.2).
 *
 * `priority` 0 is AliasMode, anything else ServiceMode. `target` defaults to `.`
 * (the root), which in ServiceMode means the owner name itself, and is how a
 * record for the origin's own host is normally written. Params are emitted in
 * ascending key order, which the RFC requires and a strict parser enforces.
 */
function httpsRdata({ priority = 1, target = ".", alpn, port } = {}) {
	const params = [];

	if (alpn !== undefined) {
		const tokens = alpn.map((token) => {
			const buf = Buffer.alloc(1 + token.length);
			buf.writeUInt8(token.length, 0);
			buf.write(token, 1, "latin1");
			return buf;
		});
		params.push({ key: SVCPARAM_ALPN, value: Buffer.concat(tokens) });
	}

	if (port !== undefined) {
		const value = Buffer.alloc(2);
		value.writeUInt16BE(port, 0);
		params.push({ key: SVCPARAM_PORT, value });
	}

	params.sort((a, b) => a.key - b.key);

	const head = Buffer.alloc(2);
	head.writeUInt16BE(priority, 0);

	const encoded = params.map(({ key, value }) => {
		const buf = Buffer.alloc(4 + value.length);
		buf.writeUInt16BE(key, 0);
		buf.writeUInt16BE(value.length, 2);
		value.copy(buf, 4);
		return buf;
	});

	// The target is written out in full: RFC 9460 forbids compressing it, and a
	// pointer here would be read as a label length.
	return Buffer.concat([head, encodeName(target), ...encoded]);
}

/**
 * SOA RDATA whose TTL and MINIMUM both carry `ttl`.
 *
 * A negative answer has no record to hang a TTL on, so RFC 2308 carries it in an
 * SOA in the authority section, and a resolver takes the lower of that record's
 * TTL and the SOA's MINIMUM. Without one, a conforming resolver declines to cache
 * the answer at all, which is why the helper sends it: a server that omitted it
 * would make every negative answer look uncacheable and any measurement of
 * negative caching an artefact of the helper.
 */
function soaRdata(ttl) {
	const nums = Buffer.alloc(20);
	nums.writeUInt32BE(1, 0); // serial
	nums.writeUInt32BE(3600, 4); // refresh
	nums.writeUInt32BE(600, 8); // retry
	nums.writeUInt32BE(604800, 12); // expire
	nums.writeUInt32BE(ttl, 16); // minimum, which bounds negative caching
	return Buffer.concat([
		encodeName("ns.dns-server.test"),
		encodeName("hostmaster.dns-server.test"),
		nums,
	]);
}

/** Read a label sequence. Questions are never compressed, so pointers are a fault. */
function decodeName(buf, offset) {
	const labels = [];
	let off = offset;
	for (;;) {
		if (off >= buf.length) throw new Error("truncated name");
		const len = buf[off];
		if (len === 0) return { name: labels.join("."), end: off + 1 };
		if ((len & 0xc0) !== 0) throw new Error("compressed name in question");
		off += 1;
		if (off + len > buf.length) throw new Error("truncated label");
		// latin1: label bytes pass through unchanged, which keeps 0x20 case
		// randomisation intact for the echo below.
		labels.push(buf.subarray(off, off + len).toString("latin1"));
		off += len;
	}
}

function skipName(buf, offset) {
	let off = offset;
	for (;;) {
		if (off >= buf.length) throw new Error("truncated name");
		const len = buf[off];
		if (len === 0) return off + 1;
		if ((len & 0xc0) === 0xc0) return off + 2;
		off += 1 + len;
	}
}

function parseQuery(buf) {
	if (buf.length < 12) throw new Error("short message");
	const id = buf.readUInt16BE(0);
	const flags = buf.readUInt16BE(2);
	if ((flags & 0x8000) !== 0) throw new Error("not a query");
	const qdcount = buf.readUInt16BE(4);
	if (qdcount !== 1) throw new Error(`unsupported question count: ${qdcount}`);
	const { name, end } = decodeName(buf, 12);
	if (end + 4 > buf.length) throw new Error("truncated question");
	const type = buf.readUInt16BE(end);
	const klass = buf.readUInt16BE(end + 2);
	const questionEnd = end + 4;

	// Walk the remaining sections for an OPT record: if the client offered EDNS,
	// the reply carries one back, and its class field is the payload size.
	let ednsPayload = null;
	const rrCount =
		buf.readUInt16BE(6) + buf.readUInt16BE(8) + buf.readUInt16BE(10);
	let off = questionEnd;
	for (let i = 0; i < rrCount && off < buf.length; i++) {
		off = skipName(buf, off);
		if (off + 10 > buf.length) break;
		const rrType = buf.readUInt16BE(off);
		const rrClass = buf.readUInt16BE(off + 2);
		const rdlength = buf.readUInt16BE(off + 8);
		if (rrType === TYPE_OPT) ednsPayload = rrClass;
		off += 10 + rdlength;
	}

	return { id, flags, name, type, klass, questionEnd, ednsPayload };
}

function buildResponse(
	request,
	query,
	{ rcode, type, addresses, rdatas, ttl, soaTtl },
) {
	// `rdatas` carries record types the helper encodes itself (HTTPS); the
	// address types are built from their strings here.
	const answers =
		rdatas ?? (addresses ?? []).map((address) => addressToRdata(address, type));
	// A negative answer carries its TTL in an authority-section SOA, so an answer
	// with no records gets one and a positive answer does not.
	const authority = answers.length === 0 && soaTtl !== undefined;
	const parts = [];

	const header = Buffer.alloc(12);
	header.writeUInt16BE(query.id, 0);
	// QR | AA | RA, echoing RD, with the rcode in the low nibble. AA because the
	// zone is served locally; RA because clients set RD and expect it honoured.
	header.writeUInt16BE(
		0x8000 | 0x0400 | 0x0080 | (query.flags & 0x0100) | (rcode & 0x0f),
		2,
	);
	header.writeUInt16BE(1, 4);
	header.writeUInt16BE(answers.length, 6);
	header.writeUInt16BE(authority ? 1 : 0, 8);
	header.writeUInt16BE(query.ednsPayload !== null ? 1 : 0, 10);
	parts.push(header);

	// Echo the question bytes verbatim. Hickory can randomise the case of the
	// name (draft-vixie-dnsext-dns0x20) and drops a reply whose name doesn't
	// match byte for byte, so re-encoding from the parsed string would break it.
	parts.push(request.subarray(12, query.questionEnd));

	for (const rdata of answers) {
		const rr = Buffer.alloc(12 + rdata.length);
		// Point at the question's name rather than repeating it.
		rr.writeUInt16BE(0xc00c, 0);
		rr.writeUInt16BE(type, 2);
		rr.writeUInt16BE(CLASS_IN, 4);
		rr.writeUInt32BE(ttl, 6);
		rr.writeUInt16BE(rdata.length, 10);
		rdata.copy(rr, 12);
		parts.push(rr);
	}

	if (authority) {
		const rdata = soaRdata(soaTtl);
		const rr = Buffer.alloc(12 + rdata.length);
		rr.writeUInt16BE(0xc00c, 0);
		rr.writeUInt16BE(TYPE_SOA, 2);
		rr.writeUInt16BE(CLASS_IN, 4);
		rr.writeUInt32BE(soaTtl, 6);
		rr.writeUInt16BE(rdata.length, 10);
		rdata.copy(rr, 12);
		parts.push(rr);
	}

	if (query.ednsPayload !== null) {
		const opt = Buffer.alloc(11);
		opt.writeUInt8(0, 0); // root name
		opt.writeUInt16BE(TYPE_OPT, 1);
		opt.writeUInt16BE(Math.max(512, query.ednsPayload), 3); // payload size
		opt.writeUInt32BE(0, 5); // extended rcode and flags
		opt.writeUInt16BE(0, 9); // rdlength
		parts.push(opt);
	}

	return Buffer.concat(parts);
}

/**
 * Start the server on an ephemeral port on `host`.
 *
 * `zone` maps a name to `{ a, aaaa, https, ttl, httpsTtl }`: `a` and `aaaa` are
 * address string arrays, `https` is an array of `{ priority, target, alpn, port }`
 * HTTPS records, and the TTLs are in seconds, defaulting to `defaultTtl`
 * (`httpsTtl` to `ttl`). A name in the zone with no records of the queried type
 * answers NOERROR with no records, the way a real name with only an A record
 * answers a AAAA query; a name absent from the zone answers NXDOMAIN.
 *
 * Returns the handle documented on the individual methods below.
 */
async function startDnsServer({
	host = "127.0.0.1",
	zone = {},
	defaultTtl = 1,
	negativeTtl = 30,
	delayMs = 0,
} = {}) {
	const records = new Map();
	const failures = new Map();
	const queries = [];
	const timers = new Set();
	let delay = delayMs;
	let closed = false;

	function setRecord(name, entry) {
		const key = normaliseName(name);
		if (entry === null) {
			records.delete(key);
			return;
		}
		const a = entry.a ?? [];
		const aaaa = entry.aaaa ?? [];
		// Parse eagerly so a malformed address is the caller's error here rather
		// than a mysterious SERVFAIL later.
		for (const address of a) ipv4ToBuffer(address);
		for (const address of aaaa) ipv6ToBuffer(address);
		// Encoded eagerly for the same reason, and because the params are fixed
		// once set: nothing about them varies per query.
		const https = (entry.https ?? []).map(httpsRdata);
		records.set(key, {
			a,
			aaaa,
			https,
			ttl: entry.ttl ?? defaultTtl,
			httpsTtl: entry.httpsTtl ?? entry.ttl ?? defaultTtl,
		});
	}

	for (const [name, entry] of Object.entries(zone)) setRecord(name, entry);

	/** Decide the reply, or `null` to drop the query. */
	function answer(query, transport) {
		const name = normaliseName(query.name);
		queries.push({
			name,
			type: TYPE_NAMES[query.type] ?? String(query.type),
			transport,
			at: Date.now(),
		});

		const entry = failures.get(name);
		// A failure scoped to one record type leaves the others answering, which is
		// how a query for one type is failed without also breaking resolution.
		const failure =
			entry && (entry.type === null || entry.type === query.type)
				? entry.mode
				: null;
		if (failure === "drop") return null;
		// SERVFAIL is not a negative answer about the name, so it carries no SOA
		// and is not cacheable; NXDOMAIN is, and does.
		if (failure === "servfail") return { rcode: RCODE_SERVFAIL };
		if (failure === "nxdomain") {
			return { rcode: RCODE_NXDOMAIN, soaTtl: negativeTtl };
		}

		if (query.klass !== CLASS_IN) return { rcode: RCODE_NOTIMP };
		if (
			query.type !== TYPE_A &&
			query.type !== TYPE_AAAA &&
			query.type !== TYPE_HTTPS
		) {
			// Not an error: a name can simply hold no records of that type, and
			// answering NOTIMP would make hickory retry rather than move on.
			return { rcode: RCODE_NOERROR, soaTtl: negativeTtl };
		}

		const record = records.get(name);
		if (!record) return { rcode: RCODE_NXDOMAIN, soaTtl: negativeTtl };

		if (query.type === TYPE_HTTPS) {
			return {
				rcode: RCODE_NOERROR,
				type: TYPE_HTTPS,
				rdatas: record.https,
				ttl: record.httpsTtl,
				// A name with addresses but no HTTPS record is the common case, and
				// that negative answer needs the SOA to be cacheable like any other.
				soaTtl: negativeTtl,
			};
		}

		return {
			rcode: RCODE_NOERROR,
			type: query.type,
			addresses: query.type === TYPE_AAAA ? record.aaaa : record.a,
			ttl: record.ttl,
			// Reached when the name holds no address of the family asked for, which
			// is a negative answer and needs the SOA to be cacheable.
			soaTtl: negativeTtl,
		};
	}

	/** Reply after the configured delay, unless the server closed meanwhile. */
	function respond(send) {
		if (delay <= 0) {
			send();
			return;
		}
		const timer = setTimeout(() => {
			timers.delete(timer);
			if (!closed) send();
		}, delay);
		timers.add(timer);
	}

	function handle(request, transport, send) {
		let query;
		try {
			query = parseQuery(request);
		} catch {
			// Malformed enough that there's no question to echo; only a message
			// with a readable id can be answered at all.
			if (request.length >= 12) {
				const header = Buffer.alloc(12);
				header.writeUInt16BE(request.readUInt16BE(0), 0);
				header.writeUInt16BE(0x8000 | RCODE_FORMERR, 2);
				respond(() => send(header));
			}
			return;
		}
		const reply = answer(query, transport);
		if (reply === null) return;
		respond(() => send(buildResponse(request, query, reply)));
	}

	let udp;
	let tcp;

	const makeUdp = () => {
		const socket = dgram.createSocket(net.isIPv6(host) ? "udp6" : "udp4");
		socket.on("message", (message, remote) => {
			handle(message, "udp", (response) => {
				socket.send(response, remote.port, remote.address, () => {});
			});
		});
		return socket;
	};

	const makeTcp = () => net.createServer((socket) => {
		// DNS over TCP prefixes each message with its length, and a message can
		// arrive split across reads.
		let buffered = Buffer.alloc(0);
		socket.on("data", (chunk) => {
			buffered = Buffer.concat([buffered, chunk]);
			for (;;) {
				if (buffered.length < 2) return;
				const length = buffered.readUInt16BE(0);
				if (buffered.length < 2 + length) return;
				const message = buffered.subarray(2, 2 + length);
				buffered = buffered.subarray(2 + length);
				handle(message, "tcp", (response) => {
					const framed = Buffer.alloc(2 + response.length);
					framed.writeUInt16BE(response.length, 0);
					response.copy(framed, 2);
					if (!socket.destroyed) socket.write(framed);
				});
			}
		});
		socket.on("error", () => socket.destroy());
	});

	// The OS picks the port by binding UDP, then TCP takes the same number. Nothing
	// reserves that number on the TCP side in between, so another listener can take it
	// first: retry on a fresh pair rather than failing whichever test asked for a server.
	//
	// Both sockets are closed before retrying, and before giving up. A UDP socket left
	// bound holds the event loop open, which would turn one failed bind into the whole
	// run hanging long after its last assertion.
	let port;
	for (let attempt = 1; ; attempt++) {
		udp = makeUdp();
		tcp = makeTcp();
		tcp.on("error", () => {});
		try {
			await new Promise((resolve, reject) => {
				udp.once("error", reject);
				udp.bind(0, host, resolve);
			});
			port = udp.address().port;
			await new Promise((resolve, reject) => {
				tcp.once("error", reject);
				tcp.listen(port, host, resolve);
			});
			break;
		} catch (err) {
			// Either may never have come up, and closing one that did not is itself an
			// error; the point here is only that neither is left holding anything.
			for (const socket of [udp, tcp]) {
				try {
					socket.close();
				} catch {
					// never came up, which is what was wanted
				}
			}
			if (attempt >= 5) throw err;
		}
	}

	return {
		host,
		port,
		/** `["ip:port"]`, the shape a nameserver list is usually configured with. */
		get servers() {
			return [`${net.isIPv6(host) ? `[${host}]` : host}:${port}`];
		},

		/** Replace a name's records, or remove it with `null`. Takes effect immediately. */
		set: setRecord,

		/**
		 * Make `name` fail: `servfail`, `nxdomain`, or `drop` (never answer, so
		 * the client times out). Pass `null` to answer from the zone again.
		 *
		 * `type` scopes the failure to one record type (`"A"`, `"AAAA"`, `"HTTPS"`),
		 * leaving the rest answering normally; omit it to fail every type.
		 */
		fail(name, mode = "servfail", { type = null } = {}) {
			const key = normaliseName(name);
			if (mode === null) {
				failures.delete(key);
				return;
			}
			const code =
				type === null
					? null
					: Number(
							Object.keys(TYPE_NAMES).find((k) => TYPE_NAMES[k] === type) ?? NaN,
						);
			if (Number.isNaN(code)) throw new Error(`unknown record type ${type}`);
			failures.set(key, { mode, type: code });
		},

		/** Change the reply delay, in milliseconds. */
		setDelay(ms) {
			delay = ms;
		},

		/** Every query received, oldest first: `{ name, type, transport, at }`. */
		get queries() {
			return queries.slice();
		},

		/** How many queries arrived for `name`, optionally of one type only. */
		countFor(name, type = null) {
			const key = normaliseName(name);
			return queries.filter(
				(q) => q.name === key && (type === null || q.type === type),
			).length;
		},

		/** Forget the query log, so a later assertion counts from here. */
		resetQueries() {
			queries.length = 0;
		},

		async close() {
			closed = true;
			for (const timer of timers) clearTimeout(timer);
			timers.clear();
			await new Promise((resolve) => udp.close(resolve));
			await new Promise((resolve) => {
				tcp.close(resolve);
				// Idle keep-alive sockets would otherwise hold the close open.
				tcp.closeAllConnections?.();
			});
		},
	};
}

module.exports = { startDnsServer };
