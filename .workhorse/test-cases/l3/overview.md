# L3 — Release tooling and first publish test cases

Coverage for the release path: independent versioning, semver-checks, MSRV in CI,
and the packaging/publishing that S1 could not exercise before the crates existed
on crates.io. Several cases can only be run once the first manual publish has
happened, so they stay unticked until then.

## MSRV is verified, not asserted (spec: RUST)

- [x] `cargo +1.96 build --workspace` succeeds, so every crate compiles under the
  declared `rust-version = "1.96"`. Verified locally.
- [x] `cargo +1.96 test --workspace` passes against a local httpbin, so the Rust
  tests hold under MSRV as well as stable. Verified locally.
- [ ] The `msrv - 1.96` job runs on pull requests and is required by the
  `Tests pass` gate, and is allowed to skip on cleanup-only commits (listed in the
  gate's `SKIPPABLE_JOBS`).

## Independent versioning (spec: RUST)

- [x] Each of the six published crates carries its own `version` in its manifest,
  not `version.workspace`, so release-plz can move one crate without the others.
- [x] `web-faith-napi` keeps `publish = false` and inherits the workspace version;
  release-plz leaves it out of the release (`release = false` in `release-plz.toml`).
- [x] The internal `[workspace.dependencies]` pins for the six crates match the
  versions their manifests declare, so a consumer resolves the declared versions.

## Semver-checks gate a breaking change (spec: RUST)

- [ ] `cargo-semver-checks` passes on a release that claims to be non-breaking
  (a patch/minor with no public-API removal).
- [ ] A deliberate breaking change to a component crate's public API is reported by
  release-plz / cargo-semver-checks as requiring a major bump rather than a patch.

## Packaging works (coverage S1 could not reach)

- [x] `cargo package -p web-faith-conn-tracker`, `-p web-faith-cookies`,
  `-p web-faith-dns`, `-p web-faith-encoding` each build a package (leaf crates,
  no internal deps). Verified locally.
- [ ] `cargo package -p web-faith-alt-svc` and `-p web-faith` build once their
  internal dependencies are resolvable by version (i.e. after those deps are
  published), verifying the `version` + `path` dep entries publish cleanly.
- [ ] `cargo publish --dry-run -p web-faith-alt-svc` and `-p web-faith` succeed
  against the published dependency versions.

## Published crates resolve for a consumer (coverage S1 could not reach)

- [ ] A fresh project that depends on `web-faith` resolves it and its component
  crates from crates.io, pulling each component at the version `web-faith` declares.
- [ ] The four leaf crates each resolve standalone in a fresh project without
  pulling `web-faith`.

## The npm package is whole (coverage S1 could not reach)

- [ ] `@passcod/faith` installs from a packed tarball (`npm pack` then install the
  tarball in a fresh project) and loads, so the published module is not missing a
  file the crate-workspace layout moved (`index.js`, `index.d.ts`, the `.node`
  binary, and any wrapper files the package's entry points expect).
