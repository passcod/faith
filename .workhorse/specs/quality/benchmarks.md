---
id: BENCH
---

# Benchmark suite

The performance claim in the README is backed by a methodology-driven benchmark harness in `bench/`, kept as its own package so benchmark dependencies never enter the library's manifest.
Its outputs are the charts committed to the repository and embedded in the README.

## Methodology

Comparisons run against at most one representative per transport stack and API style, so wrappers over already-represented stacks don't pad the field.
All measurement happens against local in-process servers with deterministic payloads; warmup samples are discarded; time-to-first-byte is measured separately from full body drain; every implementation consumes bodies identically; and event-loop delay is recorded alongside latency so throughput wins can't hide loop stalls.
Known unfairnesses are documented rather than corrected invisibly (e.g. a competitor whose buffering makes its time-to-first-byte equal its total; cold-agent numbers dominated by trust store loading).
Node has no HTTP/3 server, so a standalone Rust server serves identical routes and payloads for HTTP/3 scenarios.

## Suites

Named suites scale from a quick sanity pass to the full matrix of implementations × protocol (HTTP/1 plain and TLS, HTTP/2, HTTP/3) × payload size × concurrency × warm/cold, plus a concurrency sweep for the throughput curve.
A features suite benchmarks Fáith against itself one option at a time (protocol, DNS resolver choice, IP family, cache store, cookies), quantifying what each feature costs.
Results are written as newline-delimited JSON, and a plotting script renders the committed SVG charts with fixed per-implementation colours so a series is recognisable across charts.
