const http = require("http");

function createConnectionTracker(options = {}) {
	const connections = new Map();
	let connectionCounter = 0;
	let requestCounter = 0;

	const server = http.createServer((req, res) => {
		const socketId = req.socket.__trackerId;
		const requestId = ++requestCounter;

		const conn = connections.get(socketId);
		if (conn) {
			conn.requests.push({
				id: requestId,
				method: req.method,
				url: req.url,
				at: Date.now(),
			});
		}

		if (req.url === "/get") {
			res.writeHead(200, { "Content-Type": "application/json" });
			res.end(JSON.stringify({ requestId, socketId, message: "ok" }));
		} else if (req.url.startsWith("/delay/")) {
			const ms = parseInt(req.url.split("/")[2], 10) || 100;
			setTimeout(() => {
				res.writeHead(200, { "Content-Type": "application/json" });
				res.end(JSON.stringify({ requestId, socketId, delayed: ms }));
			}, ms);
		} else if (req.url.startsWith("/bytes/")) {
			const count = parseInt(req.url.split("/")[2], 10) || 100;
			res.writeHead(200, { "Content-Type": "application/octet-stream" });
			res.end(Buffer.alloc(count, "x"));
		} else if (req.url.startsWith("/stream/")) {
			const chunks = parseInt(req.url.split("/")[2], 10) || 5;
			const delayMs = parseInt(req.url.split("/")[3], 10) || 50;
			res.writeHead(200, { "Content-Type": "application/octet-stream" });

			let sent = 0;
			const interval = setInterval(() => {
				res.write(Buffer.alloc(100, "x"));
				sent++;
				if (sent >= chunks) {
					clearInterval(interval);
					res.end();
				}
			}, delayMs);
		} else if (req.url === "/status/204") {
			res.writeHead(204);
			res.end();
		} else {
			res.writeHead(404);
			res.end("Not Found");
		}
	});

	server.on("connection", (socket) => {
		const id = ++connectionCounter;
		socket.__trackerId = id;
		connections.set(id, {
			id,
			requests: [],
			openedAt: Date.now(),
			closedAt: null,
		});
		socket.on("close", () => {
			const conn = connections.get(id);
			if (conn) {
				conn.closedAt = Date.now();
			}
		});
	});

	server.keepAliveTimeout = options.keepAliveTimeout ?? 5000;
	server.headersTimeout = options.headersTimeout ?? 60000;

	return {
		server,

		listen() {
			return new Promise((resolve, reject) => {
				server.listen(0, "127.0.0.1", (err) => {
					if (err) reject(err);
					else resolve(server.address().port);
				});
			});
		},

		close() {
			return new Promise((resolve) => {
				server.close(() => resolve());
			});
		},

		stats() {
			const allConnections = [...connections.values()];
			return {
				totalConnections: connectionCounter,
				activeConnections: allConnections.filter((c) => !c.closedAt).length,
				closedConnections: allConnections.filter((c) => c.closedAt).length,
				totalRequests: requestCounter,
				connections: allConnections,
			};
		},

		reset() {
			connections.clear();
			connectionCounter = 0;
			requestCounter = 0;
		},

		url(path) {
			const addr = server.address();
			return `http://${addr.address}:${addr.port}${path}`;
		},
	};
}

module.exports = { createConnectionTracker };
