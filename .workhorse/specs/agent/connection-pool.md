---
id: POOL
---

# Connection pool

Each agent pools connections so subsequent requests to the same endpoint skip DNS, TCP, and TLS setup. Pooling is always on; the options bound how long and how many idle connections are kept.

- [ ] `pool.idleTimeout` closes a connection after that many seconds of inactivity. Default 90 seconds. The same window bounds how long an idle connection appears in `connections()` (see `agent/observability.md`).
- [ ] `pool.maxIdlePerHost` caps idle connections kept per host, closing connections to stay under it. Default: no limit.
- [ ] HTTP/1 connections return to the pool once their response body has been fully read or discarded; an unconsumed body holds its connection (see `response/reading-the-body.md`).
- [ ] HTTP/2 and HTTP/3 connections multiplex, so reuse does not depend on body consumption.
- [ ] The connection established by a successful HTTP/3 probe lands in the same pool, so the first upgraded request starts on a warm connection (see `http3/probing.md`).
