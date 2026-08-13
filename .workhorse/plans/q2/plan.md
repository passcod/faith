# Flow-control windows

Implements [FLOW](../../specs/agent/flow-control.md): a common `flowControl` option group setting
per-stream and whole-connection receive windows for HTTP/2 and HTTP/3 alike, per-protocol overrides,
and HTTP/2 adaptive windowing.

## Notes

Windows are `u32` bytes across the whole API. reqwest takes `u32` for the HTTP/2 knobs and `u64` for
the QUIC ones; a single JS-facing type keeps these plain numbers rather than BigInts, and 4 GiB is
far above any useful window.

The HTTP/3 knobs live behind the `http3` cargo feature. The `flowControl` group and the HTTP/2 knobs
do not, so the common group has to resolve without the feature too.

reqwest's `http2_adaptive_window(true)` overrides whatever window sizes were set on the builder, so
the mutual exclusion the spec describes is enforced by simply not calling the window setters when
adaptive is on. Relying on that would be implicit; skip the calls explicitly so the precedence is
readable at the call site.

## Steps

- [x] Add `AgentFlowControlOptions` (`streamWindow`, `connectionWindow`) and wire `flowControl` into `AgentOptions`
- [x] Add `AgentHttp2Options` (`streamWindow`, `connectionWindow`, `adaptiveWindow`) and wire `http2` into `AgentOptions`
- [x] Add `streamWindow`, `connectionWindow`, `sendWindow` to `AgentHttp3Options`
- [x] Resolve precedence (per-protocol over common over default) and apply to the reqwest builder
- [x] Defaults: 6 MiB stream, 15 MiB connection, applied to both protocols
- [x] Rust unit tests for the precedence resolution
- [x] JS integration test that a large transfer works on the new defaults
- [x] Regenerate `index.d.ts`, run `cargo test` and the JS suite
