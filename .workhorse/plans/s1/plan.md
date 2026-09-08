# S1 — Expose a Rust API and publish to crates.io

Restructure the single `faith` cdylib into a Cargo workspace: a browser-shaped Rust client
`web-faith`, five standalone component crates beneath it, and a thin `web-faith-napi` binding that
ships as `@passcod/faith`. Then publish to crates.io. Target architecture is specified in
[RUST](../../specs/rust/overview.md) and [RSAPI](../../specs/rust/client-api.md).

## Scope reality

This is an 8-crate restructure of ~11,500 lines, not a single focused change. It is being built on
this branch as one long-lived PR, at the user's direction, rather than split into a card breakdown.

Steps 0–13 are done: the workspace stands, the five components are out, the client owns the agent,
the request path, and the response, and the both-surfaces spec sweep has run over the tree. Every
crate but `web-faith-napi` builds with no napi in its graph. The release tooling and the first
publish to crates.io are spun off into their own card.

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
- [x] **9. Build the fetch-flavoured client API** per [RSAPI](../../specs/rust/client-api.md).
  A Rust caller now reaches everything through the client: `Agent::new`/`builder`, `agent.fetch`,
  `Request`, and the response's own reads.
  - [x] `USER_AGENT` is the client's, composed from its own version and reqwest's, which `web-faith`
        now reads in a build script of its own. The binding's constant reads from it.
  - [x] Reading a response: the accessors and `bytes`/`text`/`json`/`body_stream`/`discard`/
        `to_file`/`timing`/`trailers` on `web_faith::response::Response`, with the napi methods
        delegating. `json` is generic over what it deserialises into.
  - [x] `http_body::Body` for the body, and `Response::into_http`. Fallible, since taking the body
        can find it already being consumed.
  - [x] The option groups and the ~440-line validation moved to `web_faith::options` and
        `Agent::from_options`, so both surfaces settle defaults in one place. `Agent::new()` follows.
  - [x] **`close()` and `network_changed()` act on the agent, not the handle.** The closeable state
        sits in a `Live` behind a shared lock, so every clone sees a close, as
        [RSAPI](../../specs/rust/client-api.md) requires. A request takes its handle at the moment it
        is issued — `request::send` is given the client rather than reaching for it — which is what
        [AGENT](../../specs/agent/overview.md) means by in flight from the moment it is issued. The
        JS suite caught the difference: capturing inside the promise instead of at issue stranded a
        request that was issued just before a close.
  - [x] `Agent::builder()` with nested builders reached through a closure, so a group left alone is
        absent from the call. Setters take `Duration` and `IpAddr` rather than the units and strings
        the options carry.
  - [x] `Request`, `Request::new`, `try_clone`, and `agent.fetch(target)` over `IntoFuture`, with the
        layering rules: outermost explicit value wins, untouched settings inherit, headers merge by
        name and a removal clears what is underneath, the URL comes from the bottom of the stack.
  - [x] Setters take the canonical `http` types or anything converting into them, and a failed
        conversion is held until the builder resolves — at `build()` for a request, at the await for
        a fetch. The first failure met is the one reported.
  - [x] `Agent::cookies()` hands back the jar itself.
  - [x] `http::Request` as a target, bringing its method, URL, headers, and body across.
  - [x] The recipe structs are closed up, now that step 10 has settled the public surface:
        `ClientRecipe`, `AgentSettings`, `Live`, `BuiltClients`, `NodeEnvRecipe`, `H3UpgradeRecipe`,
        `HttpCacheRecipe`, `HttpCacheStore`, `ResolvedWindows`, `Agent::build`, and `resolve_windows`
        are `pub(crate)`, along with `Agent`'s own state fields. None of them was named outside
        `web-faith`, so a published crate no longer carries them on its semver surface.
        `Agent::connections()` is what the binding reads for per-connection reporting, rather than
        the tracker handle.
  - [x] The option structs are `#[non_exhaustive]` with `bon`-generated builders. Adding an option
        to a struct with public fields is a breaking change, which is the wrong shape for the API
        this PR designs, and `#[non_exhaustive]` alone would have left the binding unable to
        construct anything. `bon` is what resolves both: its derive expands in the defining crate,
        so the struct literal it emits is unaffected, while a caller outside reaches the fields
        through generated setters. `builder.rs` went from 596 lines to 47, keeping only the `build`
        terminal that yields an `Agent`; `bon`'s own finisher is `into_options`.
  - The **option** structs stay public, and that is not a leftover. `web-faith-napi` constructs
        `options::AgentOptions` and each group across a crate boundary, which is the seam that lets
        both surfaces settle defaults in one place (`Agent::from_options`). Closing them would mean
        rewriting the binding's conversion through the builder and losing the exhaustive destructure
        that forces a decision when a new option is added. What is still open is the semver
        question: public fields with no `#[non_exhaustive]` make adding an option a breaking change.
- [x] **10. Feature wiring** — a default-on feature per capability a build can do without; disabling
  one drops the code and the API surface it gates (compile error at the call site, not a no-op), and
  the dependency too where the capability is a crate. Component, crate, and feature are three axes
  and need not line up: a crate can be non-optional, and a feature need not map to a crate.

  `web-faith` carries `cache`, `connection-tracking`, `cookies`, `dns`, `encoding`, `http3`, and the
  `tls-aws-lc-rs`/`tls-ring` backend choice; `web-faith-alt-svc` carries `dns` for the HTTPS-record
  sink. `web-faith-napi` mirrors the set. Integrity is deliberately not a feature: it is always
  built.
- [x] **11. Rust-facing tests + examples** — per-crate examples that run against that crate alone;
  client integration tests mirroring the JS suite where it translates. Add `.workhorse/test-cases/s1/`.

  Five component examples plus the client's, and `crates/web-faith/tests/fetch.rs` covering the
  fetch-flavoured surface against a live origin. Test cases are in
  [`.workhorse/test-cases/s1/overview.md`](../../test-cases/s1/overview.md).
- [x] **12. Make the crates publishable.** `publish = false` is gone from the six crates that go to
  crates.io, so the manual first publish can be run at any time without editing a manifest first.
  `web-faith-napi` keeps the flag and says why: it ships as `@passcod/faith` on npm, its product
  being a built `.node` that a crates.io consumer has no use for.
  - [x] All six `web-faith*` names are free on crates.io. The bare `faith` is taken (an unrelated
        Bible CLI at 0.3.0), which settles the card description's open question.
  - [x] The four crates with no internal dependencies package and verify: `web-faith-cookies`,
        `web-faith-conn-tracker`, `web-faith-dns`, `web-faith-encoding`. `web-faith-alt-svc` and
        `web-faith` cannot be packaged until their path dependencies exist on crates.io, which is
        the publish order rather than a defect; their required metadata is complete.
  - **The release tooling and the first publish are their own card** (see the
    [breakdown](../../breakdowns/s1/breakdown.md)): release-plz, `cargo-semver-checks`, and MSRV CI,
    with the manual first publish done at any point while that card is in progress.
- [x] **13. The both-surfaces spec sweep**, as the closing pass over the tree — deliberately last,
  once the Rust API is settled and its names are known. The Faith-own identifiers that the two
  surfaces spell differently (camelCase methods, returned-object fields, and camelCase option leaves)
  are now named as concepts with a cross-reference, while identifiers byte-identical across surfaces
  stay: the stable error codes, method names shared verbatim (`stats()`, `connections()`,
  `resolvers()`, `preconnect()`, `close()`), standard Web/DOM/fetch type names, RFC terms, and
  Resource-Timing/Server-Timing field names (including Faith's own `reused`/`requestSent` additions
  that live among them). The response specs took the light touch the reconnaissance called for
  (`bodyUsed`/`statusText` named as concepts; the Node body-method surface left as the Node
  narrative, with the Rust reads in [RSAPI](../../specs/rust/client-api.md)). The one heading rename
  (`## prefetchDns` → `## DNS prefetch`) updated its lone code anchor in `warm_up.rs`, and two
  `web-faith` build comments dropped the JS spellings (`prefetch_dns`, `network_changed`).

  The sweep went beyond the reconnaissance's enumerated files — which the recon flagged as a rough
  survey, not a closed set — to keep the treatment consistent: `tls`, `cache`, `quirks`, and the
  HTTP/3 trio (`upgrade`, `probing`, `transport`) carry the same category of camelCase option leaves,
  and leaving them JS-spelled while de-JS'ing `dns.serveStale` would have been a half-done pass. The
  single-word option paths shared verbatim across surfaces (`dns.servers`, `dns.system`, `cache.mode`,
  `http3.hints`, `tls.identity`, `tls.required`, and the like) were deliberately kept as shared
  vocabulary. `cargo build` and `cargo fmt --check` are green; the changes are markdown plus
  comment-only edits, so tests and the napi build are untouched.

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
- **`bon` fits the option structs; the request builder is not a candidate.** The request builder
  carries layering (outermost wins, a removal reaches through what is beneath), which a generated
  setter cannot express, and its setters are deliberately re-callable. `bon` rejects setting a
  member twice, which is also why the agent builder's append-style setters became plural iterator
  setters. Three things had to be checked rather than assumed, and all three hold: a
  `#[non_exhaustive]` struct still derives a builder, `#[builder(with = ...)]` gives a setter a
  different parameter type than the field (which is how `Duration` survives on fields carrying
  millis), and a `with` closure may take `impl FnOnce(GroupBuilder) -> Group`, which is what keeps
  the nested groups reachable through a closure. The one visible change is that the closure now
  ends in `.build()`.
- **A typestate builder cannot be assembled conditionally.** Each setter returns a different type,
  so a `#[cfg]` cannot sit on a call mid-chain. The way through is a `#[cfg]`-gated shadowing `let`,
  attributes being allowed on statements: the binding fills each feature-gated group that way.
- **Where a unit conversion lives moved to the boundary.** The client's setters speak `Duration`, so
  the binding converts the millis and seconds JavaScript sends. `local_address` went the same way:
  the option is an `IpAddr` rather than a string that might parse as one, and the binding is where
  a malformed one is refused, which is what the JS suite asserts.
- **Closing up visibility is a way to find dead code.** Making the recipe structs `pub(crate)` let
  rustc see two fields it could not judge while they were `pub`: `AgentSettings.h3_upgrade_enabled`
  was written from `recipe.h3_upgrade.enabled` and never read, because `build` goes to the recipe
  directly, and `ClientRecipe.dns_system` has no reader without the `dns` feature. Both are gone.
  A `pub` field on a library type silences the dead-code lint, so a type that is public before it
  needs to be hides its own redundancies.
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
- **No source file past 1000 lines, tests-only files excepted.** `dns`, `alt-svc`, the client's
  agent and request paths, and the binding's agent were each split into modules along their internal
  seams.
- **Tests live as a child module of the code they exercise,** `foo/tests.rs` under `foo.rs`, not one
  flat `tests.rs` at the crate root. A child module reaches its parent's private items, so the split
  costs no visibility: the first attempt widened a dozen internals to `pub(crate)` purely for a
  crate-root test module, which is the wrong trade.
- **Splitting a file relocates doc comments as easily as it drops them.** A cut between an item and
  its doc block leaves the block dangling at the end of one file and the item bare at the start of
  the next, which rustc catches, and a `// spec:` line under a doc block extends how far back the
  block starts. Intra-doc links break more quietly: `[`X`]` that resolved within one file needs
  `crate::X` once `X` is a sibling module away, and `cargo doc --workspace --no-deps` is what says so.
- **A feature drops the reqwest feature behind it too.** `cookies`, `dns`, `http3`, and the TLS
  backend each own their reqwest counterpart, which means those came out of the workspace root's
  `reqwest` feature list: a feature that leaves the dependency linked has not dropped anything.
  Cargo refuses a member override of a workspace dependency's `default-features`, so
  `web-faith-alt-svc` needs `default-features = false` set at the root, as `web-faith` already did.
- **The TLS backend resolves by priority, not by refusal.** Cargo features are additive, so a
  build with both `tls-aws-lc-rs` and `tls-ring` — which is what `--all-features` and any `http3`
  build are — has to mean something rather than fail. aws-lc-rs wins, and ring is installed as the
  process provider only where it is the sole choice. A `compile_error!` remains for *neither*, which
  is a real misconfiguration: an HTTPS client that cannot speak TLS is not one. reqwest's `http3`
  pins its QUIC stack to aws-lc-rs, which is why `http3` enables that backend rather than tolerating
  either.
- **napi's derives ignore `#[cfg]` on a field or an impl method.** `#[napi(object)]` re-emits field
  types and `#[napi] impl` enumerates method names, both at macro time, so a gated field or method
  leaves generated code referencing an item that is no longer there. Methods can still be removed —
  by moving them to their own `#[cfg]`-gated `#[napi] impl` block, which napi accepts — but an
  option object keeps its full shape whatever the build. So the binding refuses what it cannot
  honour instead: `refuse_absent_capabilities` for an agent option group, and a check in
  `faith_fetch` for a per-request one. Silently ignoring the option was the alternative, and it
  would make a slim build look like it worked.
- **Gating an option group means gating the tests that set it.** `cargo test` on a slim build was
  broken by tests and a doctest reaching for fields that are no longer compiled. The doctest was the
  worse of the two, having no per-feature escape: the fix was to illustrate with a group that is
  never gated. Check the matrix with `cargo test`, not just `cargo build`.
- **A slim binding gets a smaller bargain than the full suite, not the same one.** The JS suite
  assumes every capability is present, so "each feature combination passes it" is not a coverage
  goal that can be met: `compression`, `timing`, and `dns-server` all need a gated feature even
  where they never name one. What a slim build is held to instead is `test/slim/`, run against a
  `--no-default-features` binding by `npm run test:slim`: it loads, serves a request, has lost the
  methods whose capability is gone, and refuses each absent option group by name. That is the only
  thing that actually exercises `refuse_absent_capabilities` end to end.
- **`into_http` after `body_stream()` is allowed, and that is the design.** The body is a
  `SharedStream`, so both consumers see the whole body rather than one taking it from the other;
  the refusal fires only while a read holds the body lock. A test asserting the opposite was written
  and had to be replaced: the JS suite already covers the sharing, on its own side.
- **An integration test that needs an origin skips rather than fails.** `crates/web-faith/tests/fetch.rs`
  reads `HTTPBIN_URL` with no default and reports the skip when it is unset, so `cargo test` works
  on a machine with no server to hand while CI gets the full run. The JS suite defaults to
  `localhost:8888` instead, which is why it cannot be run without one.
- **Examples are the proof a component crate stands alone.** Each names only its own crate, so a
  dependency that had leaked upward would not compile. Two API gaps surfaced from writing them:
  `AltSvcCacheConfig` had no `Default`, which made the store unusable without copying eleven fields
  out of `web-faith`, and `ConnectionTracker::new` needs a tokio runtime (it spawns the counter
  refresh), which the example now says out loud.
- **go-httpbin runs as a native binary, not only a container.** `podman run ghcr.io/mccutchen/go-httpbin`
  fails on a host with no `/etc/subuid` range, because the image wants uid 65532 and unpacking it
  chowns to that. `~/go/bin/go-httpbin -port 8888` needs no root and serves the same endpoints. Note
  its header values arrive as arrays (`{"Name": ["value"]}`), and it echoes a body with no
  `Content-Type` back as a base64 data URL.
- **The binding carried two dozen dependencies it no longer used**, left over from before the
  extraction — including `web-faith-dns` and `web-faith-alt-svc`, which a feature claimed to drop
  while linking them anyway. Worth re-checking after any extraction: `use` roots in the source
  against the manifest.

## Step 13: the both-surfaces spec sweep

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
