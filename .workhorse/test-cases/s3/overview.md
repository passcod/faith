# Internals outside the semver check

## Build

- [x] web-faith builds, lints and tests without `unstable-internals`
- [x] web-faith builds, lints and tests with `unstable-internals`, including the tests gated on it
- [x] The napi crate builds with `unstable-internals`, and the Node test suite passes

## Semver check (verifies spec: RUST)

- [x] Against 1.0.1 with default features, the only failure is `feature_missing` for the old `internals` name
- [x] Against 1.0.1 with `internals` mapped to `unstable-internals`, the only failures are R3's internals body items and `feature_missing`
- [ ] The release PR regenerated after merge proposes web-faith 1.1.0
- [ ] On the release after 1.1.0, a change behind `unstable-internals` alone passes the semver check
