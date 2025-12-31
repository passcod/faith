# Fáith Motivation

- OS is Linux.
- Node.js version is 25.2.1.
- Ping latency is measured with `ping`, with 30 packets.
- HTTP latency is measured with `oha`, with 1000 requests.
- Timing measurements are done with hyperfine.
- Network measurements:
  - Captured with [nsntrace](https://github.com/nsntrace/nsntrace) with public DNS.

## Test 1: just HTTP/1, no TLS, no DNS

- Target is a host on my LAN (not on the local computer), serving a static response.
- Target is serving plaintext HTTP/1.1 (TCP, no TLS).
- Target address is provided as IPv4 + PORT.
- Ping latency is 0.24ms, mdev 0.033ms.
- HTTP response latency is 87ms average, 99ms at 90th percentile.
- Hits per test: 10.

Timings (100 hits per test):

```
Benchmark 1: node native-fetch.js
  Time (mean ± σ):     362.3 ms ±  38.9 ms    [User: 233.3 ms, System: 34.7 ms]
  Range (min … max):   310.8 ms … 413.1 ms    10 runs

Benchmark 2: node node-fetch.js
  Time (mean ± σ):     362.8 ms ±  46.4 ms    [User: 220.1 ms, System: 22.6 ms]
  Range (min … max):   317.0 ms … 469.9 ms    10 runs

Benchmark 3: node faith.js
  Time (mean ± σ):     268.1 ms ±  18.5 ms    [User: 83.8 ms, System: 22.7 ms]
  Range (min … max):   230.4 ms … 296.8 ms    11 runs

Summary
  node faith.js ran
    1.35 ± 0.17 times faster than node native-fetch.js
    1.35 ± 0.20 times faster than node node-fetch.js
```

Network (10 hits):

- Native fetch: 44 TCP packets
  - Multiplexing over two connections
- Node-fetch: 37 TCP packets
  - Multiplexing over one connection
- Fáith: 37 TCP packets
  - Multiplexing over one connection

## Test 2: just HTTP/1 + TLS

- Target is an AWS CloudFront distribution, serving a static asset.
- Target is serving HTTP/1.1 (TCP) with TLS 1.3.
- Target address is provided as a domain name, resolvable to an IPv4 only.
- Ping latency is 4.8ms, mdev 0.16ms.
- HTTP response latency is 8.7ms average, 7.4ms at 90th percentile.

Timings (100 hits per test):

```
Benchmark 1: node native-fetch.js
  Time (mean ± σ):     956.6 ms ±  66.5 ms    [User: 433.0 ms, System: 62.3 ms]
  Range (min … max):   813.7 ms … 1023.1 ms    10 runs

Benchmark 2: node node-fetch.js
  Time (mean ± σ):     951.3 ms ±  73.2 ms    [User: 422.6 ms, System: 44.7 ms]
  Range (min … max):   810.1 ms … 1081.9 ms    10 runs

Benchmark 3: node faith.js
  Time (mean ± σ):      5.492 s ±  0.884 s    [User: 0.272 s, System: 0.109 s]
  Range (min … max):    4.140 s …  6.426 s    10 runs

Summary
  node node-fetch.js ran
    1.01 ± 0.10 times faster than node native-fetch.js
    5.77 ± 1.03 times faster than node faith.js
```

Uh oh.

Network (10 hits):

- Node-fetch: 64 TCP+DNS packets
  - Multiplexing over one connection
- Fáith: 259 TCP+DNS packets
  - Not multiplexing at all.
  - Repeatedly doing AAAA DNS queries for the start name.

What seems to happen is that if we have nothing for AAAA, we don't cache that fact
(which is a reasonable choice, we don't want to cache NXDomain in general), and that
leads to performing the Happy Eyeballs again and again. Additionally, that creates
more and more connections instead of re-using the existing connection — something
must be going wrong in the reqwest pool logic when only IPv4 is available.

## Test 3: a site with both A and AAAA, supporting HTTP/2

Network (10 hits):

- Native fetch: 169 packets
- Node fetch: 167 packets
- Fáith: 134 packets

Here we see the better/expected behaviour. Fáith is more efficient, mostly because it's
actually using HTTP/2 while Node-fetch and Native are still stuck on HTTP/1. But Fáith
actually gets both the A and AAAA DNS responses, then makes the decision to use the IPv4
once, and then multiplexes over a single TCP+TLS connection. Node-fetch and Native both
multiplex over two connections each. I'll need to open up the TLS stream to see the actual
internal behaviour.
