# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1](https://github.com/passcod/faith/compare/web-faith-alt-svc-v1.0.0...web-faith-alt-svc-v1.0.1) - 2026-09-14

### Other

- stop restating the docs in Cargo.toml
- 1.0.1 for the six published crates
- keep spec markers out of published docs, and tidy the seams
- ResolverConfig, and docs that do not assume the Node surface
- parse the whole Accept-Encoding, and let Coding name anything
- remove one last "bounded and aged" mention
- this is obvious from the relevant signature
- lists where there were lists, and shorter method docs
- stop stamping headers in the Alt-Svc layer
- the examples say what the step is, not why it matters
- drop the ArrivalStamp trait from the public API
- AltSvcAdvertisement parses via FromStr
- explain what ArrivalStamp is for
- SLOW_FLOOR_MS is not API
- one-clause doc titles for the cache and middleware
- PathTime says what it estimates, then how
- give H3Prober and ArrivalStamp doc titles too
- rewrite the crate doc's tail, and the types it names
- split the crate doc into a paragraph per part
- RFC 7838's term is "alternative service"
- say what trying a dead alternative costs
- sweep the voice patterns out instead of waiting to be told
- all crates: deny(missing_docs), and document what that turned up
- all crates: feature labels on docs.rs, and non-exhaustive report types
- one style for every module, and plainer wording throughout
- rewrite the public docs, and stop the builder leaking AgentOptions
- keep the component crates standing alone
