---
id: RUST
---

# The Rust distribution

Faith's network stack is published to crates.io as a family of crates, with `web-faith` as the client a Rust caller reaches for and six component crates beneath it that each stand on their own.
The Node.js native module is built from the same workspace, so the two surfaces are two faces of one implementation rather than two implementations: a behaviour specified anywhere else in these specs holds on both unless that spec says otherwise.
The Rust API surface itself is specified in [RSAPI](client-api.md).

## The crate family

`web-faith` is the HTTP client.
It owns the agent, the request and response types, and the `fetch` entry point, and it draws on the component crates for the subsystems beneath it.
The name carries a `web-` prefix because the bare `faith` name on crates.io belongs to an unrelated project, and the prefix reads as the browser-shaped client the crate is.

Six component crates are published alongside it, each one useful to a caller who wants that piece without the client above it:

- `web-faith-cookies` is the cookie jar, with the storage, matching, and eviction rules in [COOK](../agent/cookies.md).
- `web-faith-dns` is the resolver, its cache, the discovery ladder, the `HTTPS` record query, and Happy Eyeballs, as in [DNS](../agent/dns.md).
- `web-faith-conn-tracker` reads live connection state from the operating system, as in [OBS](../agent/observability.md).
- `web-faith-alt-svc` is the Alt-Svc store and the HTTP/3 upgrade machinery, as in [H3UP](../http3/upgrade.md) and [PROBE](../http3/probing.md).
- `web-faith-encoding` is content coding for request and response bodies, as in [ENC](../fetch/content-encoding.md).
- `web-faith-integrity` is Subresource Integrity parsing and verification, as in [SRI](../fetch/integrity.md).

A component crate depends on another component crate where the subsystems genuinely compose, so `web-faith-alt-svc` draws on `web-faith-dns` for the resolution its probes need.
None of them depends on `web-faith`, which is what makes each one usable on its own.

`web-faith-napi` is the Node.js binding, and it ships to npm as the prebuilt native module `@passcod/faith`.
It is the one crate in the workspace carrying napi types: every other crate compiles, tests, and documents without a JavaScript runtime present, which is the test that the binding layer is genuinely separate rather than merely renamed.

## A component crate stands alone

A component crate names its own error type covering the failures that piece can produce, rather than a type shared across the family.
`web-faith` converts them into its own error as they reach it, and those conversions are what keep the code contract in [ERR](../errors/errors.md) intact across the split.

A component crate takes and returns types from `http`, `url`, `bytes`, and the other ecosystem crates it already speaks, rather than types belonging to Faith, wherever such a type exists for the job.
Where a component needs a shape from the layer above, it takes it as a generic or through `http::Extensions` rather than depending upward.

Each component crate carries its own documentation, and its examples run against that crate alone.

## Choosing what is built

Cargo features are how a subsystem is included or left out, so a build that has no use for a piece does not carry it.
Each of the six components has a feature on `web-faith` named for it, and the default set turns on the ones that make the client behave like a browser.
Turning a component's feature off drops the dependency and the behaviour it provides, and the client continues to work without it.

Features are the whole of the swapping mechanism: a caller chooses among the implementations Faith builds rather than supplying one.

## Versioning and support

Every published crate follows semantic versioning from `1.0.0`, and the crates version independently, so a change confined to one component moves that crate alone.
Releases are prepared by release-plz, and a release runs `cargo-semver-checks` against the previous version of each crate, so a breaking change reaches a major bump rather than a patch.

The minimum supported Rust version is 1.96, it is declared as `rust-version` in every published crate, and CI builds and tests against it as well as against stable, so the declaration is verified rather than asserted.

## Workspace layout

The repository is a Cargo workspace whose members live under `crates/`, one directory per crate named for it.
Shared package metadata (licence, repository, authors, edition, `rust-version`) is declared once at the workspace root and inherited, so the crates cannot drift apart on the fields that describe the same project.
The npm package is built from `crates/web-faith-napi`, and the generated `index.js` and `index.d.ts` continue to sit at the repository root where the package's entry points expect them.
