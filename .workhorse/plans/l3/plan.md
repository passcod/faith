# L3: Release tooling and the first publish to crates.io

Set up the ongoing release path for the six published crates and verify the MSRV
declaration, then do the first manual publish. Specified in RUST under "Versioning
and support" (`.workhorse/specs/rust/overview.md`).

## The six published crates, in dependency order

Four with no internal dependencies, then alt-svc, then web-faith:

1. `web-faith-conn-tracker`, `web-faith-cookies`, `web-faith-dns`, `web-faith-encoding`
2. `web-faith-alt-svc` (depends on `web-faith-dns`)
3. `web-faith` (depends on all of the above)

`web-faith-napi` keeps `publish = false` and continues to ship as `@passcod/faith`
on npm (a built `.node` binary), so it is out of scope for crates.io.

## Baseline (measured, not inferred)

- `test.yml`: 14 jobs, ~59 job-min on a representative PR run (3 hosts × 4 node
  versions). This is the expensive workflow. A naive MSRV matrix would add ~12
  cells and nearly double it, so MSRV goes in as **one lean job**, not a matrix.
- Internal deps already carry both `version` and `path` in `[workspace.dependencies]`,
  so `cargo publish` strips the path and resolves by version, needing no manifest
  surgery for packaging beyond setting versions.
- `release-plz` is installed locally; `cargo-semver-checks` is not yet.

## Decisions (confirmed)

- [x] **First-publish version: 1.0.0.** The six crates are born at 1.0.0, per the
  RUST spec's "from 1.0.0". The npm `@passcod/faith` package is also bumped to
  1.0.0 (it is in production now), so the root workspace version, inherited only by
  `web-faith-napi` and kept in step by `npm run version`, moves to 1.0.0 too.
- [x] **release-plz runs in CI with trusted publishing.** A `release-plz.yml`
  workflow opens the release PR and publishes on merge, authenticating to
  crates.io via OIDC (no long-lived token). The first publish of each crate is
  still manual (crates.io will not let trusted publishing create a new crate), and
  a trusted publisher must be registered per crate afterwards.

## Build steps

- [x] **Independent versioning.** The six published crates now carry their own
  `version = "1.0.0"`; `web-faith-napi` keeps `version.workspace` (npm-driven).
  Root workspace version and the internal `[workspace.dependencies]` pins → 1.0.0,
  and `package.json` → 1.0.0.
- [x] **release-plz config.** `release-plz.toml` at the workspace root: per-crate
  independent versioning (release-plz default), `semver_check = true`, and
  `web-faith-napi` excluded with `release = false`.
- [x] **release-plz workflow.** `.github/workflows/release-plz.yml`: a release job
  (`id-token: write`, trusted publishing) and a release-PR job, both on push to main.
- [x] **MSRV in CI.** A single lean `msrv - 1.96` job in `test.yml` runs
  `cargo +1.96 build --workspace` and `cargo +1.96 test --workspace`, added to the
  `Tests pass` gate and its `SKIPPABLE_JOBS`. Verifies MSRV rather than asserting it.
- [x] **First manual publish.** Done by the user: all six crates are on crates.io
  at **0.7.0**, which bootstraps each crate so trusted publishing can take over
  (crates.io will not let trusted publishing create a new crate). Trusted
  publishers are registered. 1.0.0 is therefore the first release-plz release.
- [x] **Crate metadata.** `documentation = "https://docs.rs/<crate>"` on all six,
  a README for `web-faith` with `readme = "README.md"`, and a docs.rs fix (below).
- [x] **Unyanked the lockfile.** `chacha20` 0.10.1 was yanked upstream; bumped to
  0.10.2. It reaches us transitively through `rand`, `hickory-*`, and reqwest.

## Coverage this owes (test cases S1 could not cover pre-publish)

- [ ] `cargo package` and `cargo publish --dry-run` for `web-faith-alt-svc` and
  `web-faith` (cannot be packaged until their deps are on crates.io at the version
  the manifests pin).
- [ ] `cargo-semver-checks` passing on a release that claims to be non-breaking.
- [ ] A fresh project resolving the published crates, with `web-faith` pulling its
  components at the versions it declares.
- [ ] `@passcod/faith` installing and loading from a packed tarball, so the
  published module is not missing a file the workspace layout moved.

## Known inaccuracy (not fixed here)

`web-faith`'s build script reads the reqwest version from `Cargo.lock`, and a
published crate ships the lock its own workspace resolved. A consumer who resolves
a newer reqwest gets a `User-Agent` naming the older one. A build script cannot see
what the consumer resolved, so this is a documented inaccuracy, not a bug to fix on
this card.


## docs.rs and the reqwest_unstable cfg

The 0.7.0 docs.rs build for `web-faith` failed (the other five are green):

    error: The `http3` feature is unstable, and requires the
    `RUSTFLAGS='--cfg reqwest_unstable'` environment variable to be set.

`web-faith`'s default features include `http3`, which turns on reqwest's `http3`,
and reqwest refuses to compile without that cfg. The workspace supplies it through
`.cargo/config.toml`, which is not part of a published crate, so docs.rs never saw
it. Fixed with `[package.metadata.docs.rs] rustc-args = ["--cfg", "reqwest_unstable"]`
in `crates/web-faith/Cargo.toml`; docs.rs applies those as RUSTFLAGS, so reqwest
gets the cfg. The fix ships with 1.0.0, so 0.7.0's docs stay broken (a rebuild
reads 0.7.0's own manifest, which lacks the metadata).

## Open question: the same cfg breaks a consumer's first build

Verified against the published 0.7.0 in a fresh project outside this workspace:
`cargo add web-faith` then `cargo check` fails with the same reqwest error. The
docs.rs metadata fixes docs.rs only; it does nothing for a consumer, who has to
set `RUSTFLAGS='--cfg reqwest_unstable'` themselves. The README now documents that,
which is accurate but still means `cargo add web-faith` does not build out of the box.

Two ways to resolve, and it wants a decision before 1.0.0:

- **Leave it.** Matches RUST's "features are on by default, so a caller who reaches
  for the crate without thinking about them gets the whole client", and the README
  carries the flag. Cost: a compile error is the first thing a new user meets.
- **Take `http3` out of the default set.** `cargo add web-faith` then builds, and
  HTTP/3 becomes an opt-in that documents the flag alongside it. This changes what
  RUST says about default features, so it is a spec change, not just a manifest one.
