# Rust API and crates.io publication

Coverage for splitting the single napi cdylib into a Cargo workspace, exposing `web-faith` as a
Rust client with component crates beneath it, and publishing the family.

An unticked box is coverage this card still owes, not a scenario that was considered and dropped.

## No regression on the Node surface

The binding's behaviour is the control for the whole restructure: the JS suite passed before the
split and must pass after it, unchanged.

- [x] The full JS suite passes (`HTTPBIN_URL=… npm run test:only`, 2111 assertions).
- [x] `index.js` and `index.d.ts` are byte-identical after a rebuild, or differ only by declaration
      order where a method moved to a gated `impl` block. Verified by diffing after
      `npm run build:debug`.
- [x] The generated TypeScript keeps the documentation napi emits from Rust doc comments: no class
      or method loses its docs to the move. Verifies spec: RUST.
- [ ] `@passcod/faith` installs and loads from a packed tarball, so the published module is not
      missing a file the workspace layout moved.

## The Rust client's shape

Verifies spec: RSAPI.

- [x] `Agent::new` builds an agent with defaults, and `Agent::builder` reaches every option group;
      a group left alone is absent rather than spelled out as absent.
- [x] `agent.fetch(url).await` sends without a separate send step (`IntoFuture`).
- [x] `Request::new(...).build()` prepares a request that carries no agent, can be sent more than
      once, and can be layered over per call site.
- [x] Layering puts the outermost explicit value in charge; headers merge by name, and a removal
      reaches through to whatever the layers beneath contributed.
- [x] `try_clone` copies a request with a buffered body and returns `None` for a stream body.
- [x] A setter taking anything convertible holds a failed conversion until the builder resolves,
      and the first failure met is the one reported.
- [x] `text()`, `json()`, and `bytes()` each read the one body; a second read reports
      `ResponseAlreadyDisturbed`.
- [x] Cloning an agent names the same agent: closing through one clone closes it for all, and a
      request issued afterwards reports `Closed`.
- [x] A request issued before a close runs to completion.
- [ ] `body_stream()` delivers chunks, and the trailers and timing promises settle after the last
      one.
- [ ] `write_to_file` writes the body and refuses an existing destination.
- [ ] `into_http` hands over an `http::Response` whose body is the undisturbed stream.

## Errors keep their codes across the split

Verifies spec: ERR.

- [x] `FaithErrorKind` is one definition on both surfaces, and `error_codes()` is generated from it
      rather than listed separately.
- [x] Every kind renders a message led by its own code, and one with no message falls back to its
      default.
- [x] A timeout reports `Timeout`, an integrity mismatch `IntegrityMismatch`, a malformed integrity
      value `InvalidIntegrity`, an unparseable URL `InvalidUrl`, and a bad method `InvalidMethod` —
      each rather than a generic `Network`.
- [x] A redirect the agent's own policy refused keeps the kind Faith chose, rather than being
      flattened into reqwest's own redirect error.

## Each component crate stands alone

Verifies spec: RUST.

- [x] Every component crate carries an example that runs against that crate alone, with no
      `web-faith` dependency: cookies, codings, connection tracking, the Alt-Svc store, and the
      resolver.
- [x] Each example compiles and runs (`cargo run -p <crate> --example <name>`).
- [x] `web-faith-alt-svc` builds and its example compiles without `web-faith-dns`, the HTTPS-record
      sink being the only thing that needed it.
- [x] No component crate has napi anywhere in its dependency graph; only `web-faith-napi` does.
- [ ] `cargo package` succeeds for each crate, so nothing depends on a path that only exists in the
      workspace.
- [ ] `cargo publish --dry-run` succeeds for the family in dependency order.

## Features drop what they name

Verifies spec: RUST.

- [x] `cargo build` and `cargo test` pass for: default, `--all-features`, TLS alone, and TLS plus
      each of `cache`, `connection-tracking`, `cookies`, `dns`, `encoding`, and `http3`
      individually, on both `web-faith` and `web-faith-napi`.
- [x] Turning a feature off removes the API that only means something with it: no cookie jar handle
      without `cookies`, no cache mode without `cache`, no request compression without `encoding`,
      no `resolvers()` without `dns`.
- [x] Turning a feature off drops the dependency too, including the reqwest feature behind it.
- [x] A slim binding refuses an option group it cannot honour at agent construction, and a
      per-request option at `fetch`, rather than ignoring it.
- [x] `tls-ring` builds and a client comes up under it, reqwest finding the installed provider
      rather than panicking.
- [x] Enabling both TLS backends resolves to aws-lc-rs rather than failing, so `--all-features` and
      any `http3` build work.
- [x] Selecting neither TLS backend is a compile error naming both options.
- [ ] Each feature combination passes the JS suite where the binding is what changed, not just
      `cargo build`.

## Publication and versioning

Verifies spec: RUST.

- [ ] `cargo-semver-checks` runs against the previous version of each crate and passes on a release
      that claims to be non-breaking.
- [ ] `rust-version` is declared in every published crate and inherited from the workspace root.
- [ ] CI builds and tests against MSRV 1.96 as well as stable, so the declaration is verified
      rather than asserted.
- [ ] release-plz prepares a release, and a change confined to one component moves that crate's
      version alone.
- [ ] The published crates resolve from crates.io in a fresh project, `web-faith` pulling its
      components at the versions it declares.

## Housekeeping the restructure owes

- [x] `cargo doc --workspace --no-deps` is warning-free, so no published crate ships a broken
      intra-doc link.
- [x] `cargo fmt --all --check` is clean.
- [x] No source file exceeds 1000 lines, files that are entirely tests excepted.
- [x] The binding's manifest names only crates it uses.
