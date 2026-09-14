# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.1](https://github.com/passcod/faith/compare/web-faith-encoding-v1.0.0...web-faith-encoding-v1.0.1) - 2026-09-14

### Other

- 1.0.1 for the six published crates
- keep spec markers out of published docs, and tidy the seams
- a unified call for requests too, and examples that use both
- cover a caller's own coding layered with one of ours
- decoding a layer edits the headers with it
- ContentEncoding is the type for the header, both ways
- simplify the request-encoding caveat
- request compression needs knowing the server takes it
- parse the whole Accept-Encoding, and let Coding name anything
- a module per side
- drop the Coding line from the crate doc
- an example per section
- the request and response sections are lists
- drop the Common heading for its one line
- list what is supported, and make AcceptEncoding answerable
- component crates: the same doc standard as alt-svc
- AltSvcAdvertisement parses via FromStr
- sweep the voice patterns out instead of waiting to be told
- all crates: deny(missing_docs), and document what that turned up
- all crates: feature labels on docs.rs, and non-exhaustive report types
- one style for every module, and plainer wording throughout
