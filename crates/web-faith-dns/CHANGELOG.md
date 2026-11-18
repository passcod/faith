# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.2](https://github.com/passcod/faith/compare/web-faith-dns-v1.0.1...web-faith-dns-v1.0.2) - 2026-11-18

### Fixed

- *(dns)* A4: keep FaithResolver UnwindSafe and RefUnwindSafe ([#115](https://github.com/passcod/faith/pull/115))
- *(dns)* A4: keep FaithResolver UnwindSafe and RefUnwindSafe
- *(lint)* Y3: clear the workspace's clippy warnings and run clippy in CI

### Other

- hand the resolver Faith's rustls config for encrypted DNS
