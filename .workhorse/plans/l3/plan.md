# L3 — Release tooling and the first publish to crates.io

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
  cells and nearly double it — so MSRV goes in as **one lean job**, not a matrix.
- Internal deps already carry both `version` and `path` in `[workspace.dependencies]`,
  so `cargo publish` strips the path and resolves by version — no manifest surgery
  needed for packaging beyond setting versions.
- `release-plz` is installed locally; `cargo-semver-checks` is not yet.

## Decisions (confirmed)

- [x] **First-publish version: 1.0.0.** The six crates are born at 1.0.0, per the
  RUST spec's "from 1.0.0". The npm `@passcod/faith` package is also bumped to
  1.0.0 (it is in production now), so the root workspace version — inherited only
  by `web-faith-napi` and kept in step by `npm run version` — moves to 1.0.0 too.
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
- [ ] **First manual publish.** Run by the user in dependency order (leaves →
  alt-svc → web-faith) with their own crates.io credentials. Not done by the agent.

## Coverage this owes (test cases S1 could not cover pre-publish)

- [ ] `cargo package` and `cargo publish --dry-run` for `web-faith-alt-svc` and
  `web-faith` (cannot be packaged while their deps are path-only — need the deps
  published first, or `--no-verify` staging).
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
