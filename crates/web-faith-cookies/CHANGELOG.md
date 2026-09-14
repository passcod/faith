# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1](https://github.com/passcod/faith/compare/web-faith-cookies-v1.0.0...web-faith-cookies-v1.0.1) - 2026-09-14

### Other

- stop restating the docs in Cargo.toml
- 1.0.1 for the six published crates
- keep spec markers out of published docs, and tidy the seams
- paths are paths, timings are best-effort, outputs are non_exhaustive
- each limit says how it is enforced
- cookies, dns: say plainly what the reqwest feature is for
- tell a Rust caller why a cookie was refused
- say what the jar does not implement, and call the limits limits
- component crates: the same doc standard as alt-svc
- AltSvcAdvertisement parses via FromStr
- all crates: deny(missing_docs), and document what that turned up
- all crates: feature labels on docs.rs, and non-exhaustive report types
- one style for every module, and plainer wording throughout
- rewrite the public docs, and stop the builder leaking AgentOptions
- keep the component crates standing alone
