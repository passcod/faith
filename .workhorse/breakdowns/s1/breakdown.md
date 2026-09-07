# Work spun off from the Rust API restructure

S1 delivers the workspace, the Rust client, its component crates, the feature wiring, and manifests
that are ready to publish. What it deliberately stops short of is the release path: the tooling that
keeps the crate family versioned, and the first publish itself.

## Release tooling and the first publish to crates.io

Set up the ongoing release path for the workspace's published crates: release-plz to prepare
releases and move each crate's version independently, `cargo-semver-checks` against the previous
version of each crate so a breaking change reaches a major bump rather than a patch, and CI that
builds and tests against the declared MSRV as well as stable, so `rust-version = "1.96"` is verified
rather than asserted.

The first publish is a manual step that can be run at any point while this card is in progress: S1
already leaves the six crates without a `publish = false` flag, so nothing needs editing first. It
goes in dependency order, the four crates with no internal dependencies first, then
`web-faith-alt-svc`, then `web-faith`. Only those six go to crates.io; `web-faith-napi` keeps its
flag and continues to ship as `@passcod/faith` on npm, its product being a built `.node` binary.

Coverage this owes, which S1 cannot cover before the crates exist on crates.io: `cargo package` and
`cargo publish --dry-run` for `web-faith-alt-svc` and `web-faith`, which cannot be packaged while
their dependencies are path-only; `cargo-semver-checks` passing on a release that claims to be
non-breaking; a fresh project resolving the published crates, with `web-faith` pulling its components
at the versions it declares; and `@passcod/faith` installing and loading from a packed tarball, so
the published module is not missing a file the workspace layout moved.

CI cost is the thing to watch. Measure it with `gh` before adding jobs rather than inferring it from
job counts: `test.yml` is already the expensive workflow, more so than `publish.yml` despite the
latter's cross-compiles and FreeBSD VM, and an MSRV job doubles a matrix if added naively.

One caveat worth carrying: `web-faith`'s build script reads the reqwest version from `Cargo.lock`,
and a published crate ships the lock its own workspace resolved. A consumer who resolves a newer
reqwest gets a `User-Agent` naming the older one. A build script cannot see what the consumer
resolved, so this is a known inaccuracy rather than something to fix here.

Specified in [RUST](../../specs/rust/overview.md) under "Versioning and support".
