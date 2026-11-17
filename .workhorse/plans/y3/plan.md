# Update all dependencies

Bring every dependency manifest in the repository up to its latest stable release, keeping the declared MSRV (1.96) and the Node floor (20) building.

## Notes

- Pre-releases stay out: rustls 0.24.0-dev and the netlink 1.0.0 release candidates are not adopted. http-cache-reqwest is already on the 1.0.0 alpha line, so it moves along that line.
- The workspace resolver respects `rust-version`, so `cargo update` only locks versions that build on 1.96; the MSRV job in CI checks it.
- `@types/node` follows the newest Node major CI tests (24), not the newest published types, so the types never describe APIs the tested runtimes lack.
- The bench harness's own dependencies already need Node 22.19 (undici 8), so its `engines` floor moves up to say so.

## Checklist

- [ ] Rust workspace: semver-compatible lockfile update
- [ ] Rust workspace: base64 0.23
- [ ] Rust workspace: sha2 0.11
- [ ] Rust workspace: strum 0.28
- [ ] Rust workspace: netlink-packet-core 0.9, netlink-packet-sock-diag 0.5, netlink-sys 0.9
- [ ] bench/h3-server: lockfile and manifest
- [ ] npm root: @napi-rs/cli, tape, @types/node (24.x); regenerate the napi bindings if the CLI changes them
- [ ] npm bench: got 16, engines floor
- [ ] GitHub Actions: bump action majors
- [ ] Build, `cargo test`, Node tests against local httpbin, `cargo fmt`, clippy
