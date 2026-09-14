# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1](https://github.com/passcod/faith/compare/web-faith-dns-v1.0.0...web-faith-dns-v1.0.1) - 2026-09-14

### Other

- stop restating the docs in Cargo.toml
- 1.0.1 for the six published crates
- keep spec markers out of published docs, and tidy the seams
- ServerSpec parse errors are a type, not a message
- ServerSpec parses via FromStr; Name inherits hickory's docs
- trim the resolver's caller-facing docs
- config is called config, and warming is just an example
- explain warming, where a reader will look for it
- serve_stale carries its own window, and Name is reachable
- ResolverReport carries types, not strings
- the titles I said I had written
- ResolverConfig, and docs that do not assume the Node surface
- an example for each way the resolver is used
- say how the resolver is configured, not why it exists
- parse the whole Accept-Encoding, and let Coding name anything
- the crate doc's sections are lists, being lists
- cookies, dns: say plainly what the reqwest feature is for
- AltSvcAdvertisement parses via FromStr
- sweep the voice patterns out instead of waiting to be told
- all crates: deny(missing_docs), and document what that turned up
- all crates: feature labels on docs.rs, and non-exhaustive report types
- one style for every module, and plainer wording throughout
- rewrite the public docs, and stop the builder leaking AgentOptions
