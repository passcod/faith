# Make the next web-faith release non-breaking

## Why release-plz calls it breaking

The release PR (#104) proposes web-faith 2.0.0 because cargo-semver-checks reports `body::Body`, `body::BodyHolder`, `body::drain_body_inner`, `Response::body_holder` and `Response::shared_stream` as removed.
All of them are public only under the `internals` feature, which the crate documents as permanently unstable and exempt from semver.

cargo-semver-checks enables every feature except those whose names look unstable: exactly `unstable`, `nightly`, `bench` or `no_std`, or a name starting with `_`, `unstable-` or `unstable_`.
`internals` matches none of those, so its API gets checked as if it were stable.
release-plz only exposes `semver_check` on or off and can't pass feature flags through.

## Decision: rename the feature to `unstable-internals`

Renaming puts the feature under a prefix the check skips, so future changes to internals no longer force a major bump.
Anyone depending on `internals` has to update their Cargo.toml, which the feature's documented exemption allows.

The 1.0.1 baseline on crates.io still has `internals`, so this one release is still flagged (the old body items, plus `feature_missing` for `internals`).
We get past that by setting the version by hand, which is how 1.0.1 went out too.
In release-plz's source (`release_plz_core` `diff.rs` / `updater.rs`, main branch), a local version higher than the registry's is kept as-is and only the changelog is updated.
The semver report still shows in the PR, but it no longer drives the bump.
From the release after this one, both baseline and current carry `unstable-internals`, which the check skips on both sides.

## Version: 1.1.0, not 1.0.2

R3 added the `drain_limit` and `drain_timeout` pool options, which is new public API, so this is a minor release even though the merge commit is typed `fix:`.

## Steps

- [ ] Rename the feature in `crates/web-faith/Cargo.toml` (`unstable-internals = ["raw-client"]`)
- [ ] Update every `cfg(feature = "internals")` / `cfg_attr(... "internals" ...)` in `crates/web-faith/src` (lib.rs, body.rs, builder.rs, error.rs, request.rs, response.rs, agent/build.rs) and the comments that name it (lib.rs, request.rs, request/parts.rs)
- [ ] Update the features table in `crates/web-faith/src/lib.rs` and `crates/web-faith/README.md`
- [ ] Switch `crates/web-faith-napi/Cargo.toml` to `features = ["unstable-internals"]`
- [ ] Set web-faith to 1.1.0 in `crates/web-faith/Cargo.toml` and the root `Cargo.toml` workspace dependency, and refresh `Cargo.lock`
- [ ] `cargo build` / `cargo test` with and without the feature, and `npm run build` for the napi crate
- [ ] Run `cargo semver-checks -p web-faith` locally to confirm that, apart from the one-off `feature_missing` for `internals`, only internals items are flagged
- [ ] After merge, check that the regenerated release PR proposes 1.1.0
