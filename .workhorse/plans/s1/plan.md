# S1 — Expose a Rust API and publish to crates.io

Restructure the single `faith` cdylib into a Cargo workspace: a browser-shaped Rust client
`web-faith`, five standalone component crates beneath it, and a thin `web-faith-napi` binding that
ships as `@passcod/faith`. Then publish to crates.io. Target architecture is specified in
[RUST](../../specs/rust/overview.md) and [RSAPI](../../specs/rust/client-api.md).

## Scope reality

This is an 8-crate restructure of ~11,500 lines, not a single focused change. It is being built on
this branch as one long-lived PR, at the user's direction, rather than split into a card breakdown.

Steps 0–8 are done: the workspace stands, the five components are out, and the client owns the agent,
the request path, and the response. Every crate but `web-faith-napi` builds with no napi in its
graph. What remains (steps 9–14) is the caller-facing API, the feature wiring, publishing, and the
spec sweep — step 9 being new design rather than relocation.

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

- `web-faith` — client: agent, request/response, `fetch`, layering, SRI. Depends on the five components.
- `web-faith-cookies` — the jar ([COOK](../../specs/agent/cookies.md)).
- `web-faith-dns` — resolver, cache, discovery ladder, HTTPS record, Happy Eyeballs ([DNS](../../specs/agent/dns.md)).
- `web-faith-conn-tracker` — live per-connection stats from the OS ([OBS](../../specs/agent/observability.md)).
- `web-faith-alt-svc` — Alt-Svc store + HTTP/3 upgrade/probing ([H3UP](../../specs/http3/upgrade.md), [PROBE](../../specs/http3/probing.md)); may depend on `web-faith-dns`.
- `web-faith-encoding` — request/response content coding ([ENC](../../specs/fetch/content-encoding.md)).
- `web-faith-napi` — the only crate with napi types; ships as `@passcod/faith`.

QUIC/TLS stay inside `web-faith` as reqwest features (aws-lc-rs default, ring alternative), not crates.

## Build order (each step ends green: `cargo build` + `cargo test` + napi `npm run build`)

- [x] **0. Workspace scaffold.** Root `[workspace]` with shared `[workspace.package]`
  (licence, repository, authors, edition, `rust-version = "1.96"`) and `[workspace.dependencies]`.
  Move the current crate to `crates/web-faith-napi`. Fix `build.rs` `Cargo.lock` path. Make
  `napi build` target the relocated crate and keep `index.js`/`index.d.ts` at repo root. Verify the
  npm build still produces a working `.node`. No behaviour change.
- [x] **1. Error core split.** Pure-Rust `FaithError`/`FaithErrorKind` (no napi) reachable by every
  crate; napi conversions live only in `web-faith-napi`. `ERROR_CODES` still generated from the one
  source ([ERR](../../specs/errors/errors.md)). Decide where the shared error core lives (likely in
  `web-faith`, with components naming their own error types that the client converts — per
  [RUST](../../specs/rust/overview.md) "A component crate stands alone").
- [x] **2. SRI** — extracted as `web-faith-integrity`, then folded back into `web-faith` as a module:
  too small for standing alone to buy a caller anything, and not something a build should switch off.
- [x] **3. Extract `web-faith-encoding`** — decouple from `crate::body::DynStream` (take a generic/`bytes` stream).
- [x] **4. Extract `web-faith-cookies`** — `url::Url`; reqwest `CookieStore` behind a feature.
- [x] **5. Extract `web-faith-dns`.**
- [x] **6. Extract `web-faith-conn-tracker`** (Linux/macOS/Windows submodules).
- [x] **7. Extract `web-faith-alt-svc`** — carry `HeadersStamp` (or take it generically); depend on `web-faith-dns`.
- [x] **8. Stand up `web-faith`** — done. The client holds the agent, the request path, the response
  and its reads, the body/timing/retry machinery, and the client recipe. `web-faith-napi` is the
  binding: JS option shapes, `AgentOptions` validation, the napi classes wrapping the client's types,
  and napi machinery (promises, streams, threadsafe functions). Nothing outside it carries napi.
  - [x] `body`, `timing` (measuring; the napi object stays behind), `retry` moved.
  - [x] The client-building machinery moved: `ClientRecipe`, `NodeEnvRecipe`, `HttpCacheRecipe`,
        `HttpCacheStore`, `H3UpgradeRecipe`, `ResolvedWindows`, `install_https_sink`, and a pure
        `RedirectPolicy` the napi `Redirect` converts into.
  - [x] **The `Agent` inversion.** Done: `web_faith::agent::Agent` owns the pool, the verbs, and the
        per-request settings; `web-faith-napi`'s `Agent` is a napi class holding a handle on it, and
        keeps the ~440 lines of `AgentOptions` validation that produce a recipe. `prefetch_dns` and
        `preconnect` return futures the binding wraps, so a refusal happens before the future exists.
  - [x] `response.rs` inverted: `web_faith::response::Response` holds the state and the reads, and
        writing a body out takes a progress closure rather than a threadsafe function.
  - [x] `fetch.rs` inverted: `web_faith::request::send` takes an agent, URL, options, body, and an
        optional abort future; `fetch.rs` is 59 lines converting a `fetch()` call into those.
  - **Left for step 9:** the recipe structs' public fields, which the builder should own the
        assembly of.
- [ ] **9. Build the fetch-flavoured client API** per [RSAPI](../../specs/rust/client-api.md).
  **In progress:**
  - [x] `USER_AGENT` is the client's, composed from its own version and reqwest's, which `web-faith`
        now reads in a build script of its own. The binding's constant reads from it.
  - [x] Reading a response: the accessors and `bytes`/`text`/`json`/`body_stream`/`discard`/
        `to_file`/`timing`/`trailers` on `web_faith::response::Response`, with the napi methods
        delegating. `json` is generic over what it deserialises into.
  - [x] `http_body::Body` for the body, and `Response::into_http`. Fallible, since taking the body
        can find it already being consumed.
  - [ ] **`Agent::builder()` — the big remaining piece.** ~13 option groups as nested builders, and
        the ~440 lines of validation currently in `web-faith-napi` that turn `AgentOptions` into a
        recipe. Move that logic rather than rewrite it: both surfaces must land on the same defaults,
        and a second implementation is how they drift. `Agent::new()` then follows, and the recipe
        structs' fields close up behind the builder.
  - [ ] `Request`, `Request::new`, `try_clone`, and the fetch builder over `IntoFuture`, with the
        layering rules (outermost wins; headers merge by name).
  - [ ] Setters taking anything convertible, holding a failed conversion until the builder resolves.
- [ ] **10. Feature wiring** — a default-on feature per capability a build can do without; disabling
  one drops the code and the API surface it gates (compile error at the call site, not a no-op), and
  the dependency too where the capability is a crate. Component, crate, and feature are three axes
  and need not line up: a crate can be non-optional, and a feature need not map to a crate.
- [ ] **11. Rust-facing tests + examples** — per-crate examples that run against that crate alone;
  client integration tests mirroring the JS suite where it translates. Add `.workhorse/test-cases/s1/`.
- [ ] **12. Publishing infra** — release-plz, `cargo-semver-checks` against previous version per crate,
  MSRV 1.96 declared in every published crate and exercised in CI alongside stable, independent
  versioning from `1.0.0`. Measure CI cost before adding jobs (see project memory).
- [ ] **13. First publish** to crates.io: the five components, then `web-faith`; `@passcod/faith`
  continues from npm via `web-faith-napi`.
- [ ] **14. The both-surfaces spec sweep**, as the closing pass over the tree — deliberately last,
  once the Rust API is settled and its names are known. See the section below for the site-by-site
  reconnaissance.

## What the extractions settled

Decisions taken while doing steps 0–7, worth not relitigating:

- **The lib is still named `faith`.** `web-faith-napi`'s `[lib] name = "faith"` keeps the artifact
  `libfaith.so`, which the release workflow's zigbuild steps copy by name.
- **The benchmark HTTP/3 server is a workspace `exclude`,** not a member: its own manifest says it
  keeps a separate lockfile so the quinn/h3 stack stays out of this graph. Adding it as a member
  would have pulled that stack in; leaving it unlisted broke it outright.
- **`FaithErrorKind` is gone from the native binding.** It was emitted only because the enum carried
  napi's attribute. The package's `exports` map admits nothing but the wrapper, so no consumer could
  reach it (verified: a deep import fails with `ERR_PACKAGE_PATH_NOT_EXPORTED`). The documented
  surface is `wrapper.js`'s `ERROR_CODES`, unchanged at 22 codes.
- **Do not route a napi enum through `macro_rules!`.** Doc comments arrive as mangled `r"` literals
  in the generated `.d.ts`, and the Rust name leaks into `index.js` alongside the `js_name`. This is
  why the kinds have a single plain definition in `web-faith` and the codes reach JS through
  `errorCodes()` instead.
- **`reqwest` integration sits behind a per-crate feature** on `web-faith-cookies` (`CookieStore`)
  and `web-faith-dns` (`Resolve`). Where a trait impl held the only path to real functionality — the
  jar's store-and-read — the logic moved to inherent methods and the impl delegates, so the crate is
  usable without reqwest rather than merely compilable.
- **`alt_svc` takes the client's timing stamp as a generic,** via an `ArrivalStamp` trait, because
  the stamp must exist in non-HTTP/3 builds where the alt-svc crate is not compiled at all.
- **`web-faith` only declares a component dependency once it uses it.** The full
  feature-per-component set is step 10.
- **`web-faith` is depended on with `default-features = false`,** set at the workspace root because
  Cargo refuses to let a member override a workspace dependency's defaults. Without this the
  binding's `http3` feature and the client's drifted: turning the binding's off left the client's on,
  and the `#[cfg]`-gated recipe fields stopped lining up. Check both configurations after touching
  features — `cargo build` and `cargo build -p web-faith-napi --no-default-features`.
- **The recipe structs carry public fields for now.** The binding assembles them directly; step 9's
  builder is what should own that assembly, at which point they can close up again.
- **Spec references go in normal comments, never in doc comments.** A `// spec:DNS#transports` line
  sits under the doc block, above the item. Doc comments are published API documentation, and a spec
  id means nothing to a reader on docs.rs.
- **A component crate's docs address an external reader,** not a Faith maintainer: what the crate is
  for and how to drive it, rather than why Faith needed it factored this way. Keep the reasoning
  where it is genuinely about the code's shape, drop the rest.
- **Keep rustdoc clean, and mind that napi doc comments reach TypeScript.** `cargo doc --workspace
  --no-deps` is warning-free; making the components public surfaced several links to private items,
  which would have shipped as broken docs.rs pages. Rust intra-doc syntax in a `web-faith-napi` doc
  comment is emitted verbatim into `index.d.ts`, where `[`X`]` means nothing, so plain backticks
  belong on anything a napi item documents.

## Step 14: the both-surfaces spec sweep

A spec should be one of three things, never a fourth: generic to both surfaces (naming the concept
and linking to where it is defined), specified at the correct site, or explicitly about one surface
so the reader knows which. What it should not be is shared behaviour spelled in one surface's
identifiers, which is what a JavaScript name in a spec covering both amounts to.

It runs last because the Rust names it will cite are step 9's to settle: sweeping earlier would mean
guessing at them, and a spec that cites a name which then changes is worse than one that has not
been swept yet.

[FAITH](../../specs/overview.md) and [ENV](../../specs/environment/variables.md) are done. The
remaining sites, from a survey of JS-cased identifiers:

- `agent/observability.md` (~20: `bodiesStarted`, `responseCount`, `rttUs`, and the rest of the
  per-connection fields), `agent/warm-up.md` (~10, `prefetchDns` five times),
  `agent/cookies.md` (~8), `agent/dns.md` (~6), `agent/overview.md` (~5),
  `agent/flow-control.md` (~4), `agent/connection-pool.md` (~3), and scattered singles.
- `response/response.md` and `response/reading-the-body.md` need judgment rather than a sweep: most
  of their identifiers are Resource Timing's own field names (`fetchStart`, `requestStart`), which
  are standard vocabulary and should stay. Faith's own (`bodyUsed`, `statusText`) are the ones that
  want naming as concepts, [RSAPI](../../specs/rust/client-api.md) spelling them `body_used` and
  `status_text`.
- `rust/client-api.md` needs nothing: its `Request` and `Response` are the Rust types.

So the raw identifier count overstates the work — a blind sweep would churn standard-defined names
and Rust types alike. Roughly 60 genuine sites across about ten files, each needing a decision on
whether the surrounding spec is shared or single-surface.

## Verification discipline

Every step must leave `cargo build`, `cargo test`, and the napi `npm run build` green
(tests via `HTTPBIN_URL=http://localhost:8888`, `NODE_ENV=development` for `npm install`). A component
crate's separateness is only proven when `cargo test -p <crate>` passes with no JS runtime present.
