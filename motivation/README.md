# Fáith Motivation

- OS is Linux.
- Node.js version is 25.2.1.
- Ping latency is measured with `ping`, with 30 packets.
- HTTP latency is measured with `oha`, with 1000 requests.
- Timing measurements are done with hyperfine.
- Network measurements are done with wireshark by capturing within a podman container.

## Test 1: HTTP/1 with IPv4 DNS

Timings (10 hits per test):

```
Benchmark 1: node native-fetch.js
  Time (mean ± σ):     804.6 ms ±  39.8 ms    [User: 192.0 ms, System: 37.1 ms]
  Range (min … max):   771.6 ms … 896.7 ms    10 runs

Benchmark 2: node node-fetch.js
  Time (mean ± σ):     673.9 ms ±  24.7 ms    [User: 130.7 ms, System: 19.6 ms]
  Range (min … max):   643.4 ms … 709.8 ms    10 runs

Benchmark 3: node faith.js
  Time (mean ± σ):      1.487 s ±  0.020 s    [User: 0.109 s, System: 0.029 s]
  Range (min … max):    1.454 s …  1.516 s    10 runs

Benchmark 4: /home/.cargo/build/55/828057b608385a/target/debug/minimal
  Time (mean ± σ):     756.0 ms ± 107.8 ms    [User: 75.9 ms, System: 15.4 ms]
  Range (min … max):   674.9 ms … 964.5 ms    10 runs

Summary
  node node-fetch.js ran
    1.12 ± 0.17 times faster than /home/.cargo/build/55/828057b608385a/target/debug/minimal
    1.19 ± 0.07 times faster than node native-fetch.js
    2.21 ± 0.09 times faster than node faith.js
```

Network (10 hits):

- Native fetch: TCP packets
- Node-fetch: TCP packets
- Fáith: TCP packets
- Minimal: TCP packets

## Test 2: HTTP/2 with dualstack DNS

Timings:

```
Benchmark 1: node native-fetch.js
  Time (mean ± σ):     749.3 ms ±  26.8 ms    [User: 204.1 ms, System: 36.4 ms]
  Range (min … max):   708.8 ms … 803.1 ms    10 runs

Benchmark 2: node node-fetch.js
  Time (mean ± σ):     749.7 ms ±  31.1 ms    [User: 172.8 ms, System: 30.8 ms]
  Range (min … max):   697.9 ms … 790.0 ms    10 runs

Benchmark 3: node faith.js
  Time (mean ± σ):     639.9 ms ±  31.5 ms    [User: 105.1 ms, System: 29.6 ms]
  Range (min … max):   592.6 ms … 711.7 ms    10 runs

Benchmark 4: /home/.cargo/build/55/828057b608385a/target/debug/minimal
  Time (mean ± σ):     610.0 ms ±  20.9 ms    [User: 76.8 ms, System: 14.5 ms]
  Range (min … max):   576.6 ms … 634.5 ms    10 runs

Summary
  /home/.cargo/build/55/828057b608385a/target/debug/minimal ran
    1.05 ± 0.06 times faster than node faith.js
    1.23 ± 0.06 times faster than node native-fetch.js
    1.23 ± 0.07 times faster than node node-fetch.js
```

Network:
