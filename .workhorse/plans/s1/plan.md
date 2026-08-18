# S1 — Expose a Rust API and publish to crates.io

Restructure the single `faith` cdylib into a Cargo workspace: a browser-shaped Rust client
`web-faith`, six standalone component crates beneath it, and a thin `web-faith-napi` binding that
ships as `@passcod/faith`. Then publish to crates.io. Target architecture is specified in
[RUST](../../specs/rust/overview.md) and [RSAPI](../../specs/rust/client-api.md).

## Scope reality

This is a multi-week, 8-crate restructure of ~11,500 lines, not a single focused change. The
work almost certainly wants to become a **card breakdown** (one card per crate extraction plus a
publishing card), because a plan of this size does not survive as one green PR on `s1` and, per the
workspace norm, only breakdown entries survive a merge as real cards. The steps below are the build
order whichever way we sequence it — either as the checklist for a single long-lived branch, or as
the spine for the breakdown.

**Sequencing decision needed from the user before grinding the tree** (see the chat message).

## What the discovery turned up

Facts that shape the order and difficulty:

- **The error type is napi-coupled at the base.** `src/error.rs` derives `#[napi(string_enum)]` on
  `FaithErrorKind` and holds napi conversions. `integrity` already depends on it. Splitting a
  pure-Rust error core from the napi conversion layer is a prerequisite for every component that
  reports errors, and for the client itself. This is the load-bearing first move.
- **Component modules are mostly napi-free already.** `alt_svc`, `body`, `cookies`, `dns`,
  `encoding`, `integrity`, `retry` have zero `#[napi]`. The coupling concentrates in `agent.rs`
  (52), `response.rs` (40), `options.rs` (17), `error.rs` (13), `stream_body.rs` (11), `fetch.rs`.
- **Internal component coupling is small and matches the spec's allowed shape:** `integrity → error`,
  `encoding → body::DynStream`, `alt_svc → timing::HeadersStamp` and `alt_svc → dns` (the spec
  explicitly allows `web-faith-alt-svc → web-faith-dns`). `cookies` and `dns` have no internal deps.
- **`cookies` is bound to reqwest.** It implements `reqwest::cookie::CookieStore` and takes
  `reqwest::Url`. A standalone `web-faith-cookies` should speak `url::Url` and put the reqwest
  `CookieStore` impl behind a `reqwest` feature (or move the adapter into the client).
- **The napi build resists a naive relocation.** `build.rs` reads `Cargo.lock` by the relative path
  `"Cargo.lock"`; under a workspace the lock is at the root, so this must become
  `CARGO_MANIFEST_DIR`/workspace-root aware. `napi build --platform` reads `package.json`'s `napi`
  config and builds the crate in cwd; moving the crate means teaching the napi CLI where the crate
  is and keeping generated `index.js`/`index.d.ts` at the repo root (the spec requires them there).
  `.cargo/config.toml` (`reqwest_unstable`, cross linkers) applies workspace-wide and can stay at root.

## Target crate family

- `web-faith` — client: agent, request/response, `fetch`, layering. Depends on the six components.
- `web-faith-cookies` — the jar ([COOK](../../specs/agent/cookies.md)).
- `web-faith-dns` — resolver, cache, discovery ladder, HTTPS record, Happy Eyeballs ([DNS](../../specs/agent/dns.md)).
- `web-faith-conn-tracker` — live per-connection stats from the OS ([OBS](../../specs/agent/observability.md)).
- `web-faith-alt-svc` — Alt-Svc store + HTTP/3 upgrade/probing ([H3UP](../../specs/http3/upgrade.md), [PROBE](../../specs/http3/probing.md)); may depend on `web-faith-dns`.
- `web-faith-encoding` — request/response content coding ([ENC](../../specs/fetch/content-encoding.md)).
- `web-faith-integrity` — SRI parse + verify ([SRI](../../specs/fetch/integrity.md)).
- `web-faith-napi` — the only crate with napi types; ships as `@passcod/faith`.

QUIC/TLS stay inside `web-faith` as reqwest features (aws-lc-rs default, ring alternative), not crates.

## Build order (each step ends green: `cargo build` + `cargo test` + napi `npm run build`)

- [ ] **0. Workspace scaffold.** Root `[workspace]` with shared `[workspace.package]`
  (licence, repository, authors, edition, `rust-version = "1.96"`) and `[workspace.dependencies]`.
  Move the current crate to `crates/web-faith-napi`. Fix `build.rs` `Cargo.lock` path. Make
  `napi build` target the relocated crate and keep `index.js`/`index.d.ts` at repo root. Verify the
  npm build still produces a working `.node`. No behaviour change.
- [ ] **1. Error core split.** Pure-Rust `FaithError`/`FaithErrorKind` (no napi) reachable by every
  crate; napi conversions live only in `web-faith-napi`. `ERROR_CODES` still generated from the one
  source ([ERR](../../specs/errors/errors.md)). Decide where the shared error core lives (likely in
  `web-faith`, with components naming their own error types that the client converts — per
  [RUST](../../specs/rust/overview.md) "A component crate stands alone").
- [ ] **2. Extract `web-faith-integrity`** — own error type, own docs, `cargo test -p web-faith-integrity` with no JS runtime.
- [ ] **3. Extract `web-faith-encoding`** — decouple from `crate::body::DynStream` (take a generic/`bytes` stream).
- [ ] **4. Extract `web-faith-cookies`** — `url::Url`; reqwest `CookieStore` behind a feature.
- [ ] **5. Extract `web-faith-dns`.**
- [ ] **6. Extract `web-faith-conn-tracker`** (Linux/macOS/Windows submodules).
- [ ] **7. Extract `web-faith-alt-svc`** — carry `HeadersStamp` (or take it generically); depend on `web-faith-dns`.
- [ ] **8. Stand up `web-faith`** — move agent/request/response/fetch/options here as a pure-Rust
  client; component crates converted into it at the boundary. Reduce `web-faith-napi` to the binding
  over `web-faith`.
- [ ] **9. Build the fetch-flavoured client API** per [RSAPI](../../specs/rust/client-api.md):
  `Agent`/`Agent::builder()`, cheap-clone shared agent, `agent.fetch(target) -> IntoFuture` builder
  (`#[must_use]`), `Request`/`Request::new`/`try_clone`, layering rules, `http`/`url`/`bytes` types,
  `http_body::Body` response + `Into<http::Response>`, feature-gated API surface, Tokio, drop-cancels.
- [ ] **10. Feature wiring** — one default-on feature per component on `web-faith`; disabling one
  drops the dep, the code, and the API surface it gates (compile error at the call site, not a no-op).
- [ ] **11. Rust-facing tests + examples** — per-crate examples that run against that crate alone;
  client integration tests mirroring the JS suite where it translates. Add `.workhorse/test-cases/s1/`.
- [ ] **12. Publishing infra** — release-plz, `cargo-semver-checks` against previous version per crate,
  MSRV 1.96 declared in every published crate and exercised in CI alongside stable, independent
  versioning from `1.0.0`. Measure CI cost before adding jobs (see project memory).
- [ ] **13. First publish** to crates.io: the six components, then `web-faith`; `@passcod/faith`
  continues from npm via `web-faith-napi`.

## Verification discipline

Every step must leave `cargo build`, `cargo test`, and the napi `npm run build` green
(tests via `HTTPBIN_URL=http://localhost:8888`, `NODE_ENV=development` for `npm install`). A component
crate's separateness is only proven when `cargo test -p <crate>` passes with no JS runtime present.
