# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1](https://github.com/passcod/faith/compare/web-faith-v1.0.0...web-faith-v1.0.1) - 2026-09-14

### Other

- stop restating the docs in Cargo.toml
- 1.0.1 for the six published crates
- install with cargo add, so no version is quoted
- keep spec markers out of published docs, and tidy the seams
- paths are paths, timings are best-effort, outputs are non_exhaustive
- FaithError keeps its fields, like io::Error does
- ServerSpec parse errors are a type, not a message
- ServerSpec parses via FromStr; Name inherits hickory's docs
- serve_stale carries its own window, and Name is reachable
- ResolverReport carries types, not strings
- ResolverConfig, and docs that do not assume the Node surface
- decoding a layer edits the headers with it
- ContentEncoding is the type for the header, both ways
- parse the whole Accept-Encoding, and let Coding name anything
- a module per side
- list what is supported, and make AcceptEncoding answerable
- the same doc standard as the component crates
- stop stamping headers in the Alt-Svc layer
- drop the ArrivalStamp trait from the public API
- AltSvcAdvertisement parses via FromStr
- rewrite the crate doc's tail, and the types it names
- sweep the voice patterns out instead of waiting to be told
- all crates: deny(missing_docs), and document what that turned up
- all crates: feature labels on docs.rs, and non-exhaustive report types
- one style for every module, and plainer wording throughout
- rewrite the public docs, and stop the builder leaking AgentOptions
- collapse the single-item modules, fix the doc warnings
- cut the public API down to what it should be
- say what a Rust caller does, not what a JS caller does
- align web-faith crate docs with its README
