# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0](https://github.com/passcod/faith/releases/tag/web-faith-revocation-v1.0.0) - 2026-10-23

### Fixed

- *(revocation)* U3: skip empty polls, bound future manifests, resolve relative cache-dir
- *(ci)* refuse extra roots on Android, and tamper with a cache copy in the revocation test
- *(revocation)* end paths at self-issued certs, time out the poller, tag sessions by verified epoch

### Other

- update revocation spec, update 32 files
- update 17 files
- expose revocation checking on the Node surface
- check revocation on every agent's connections
- test revocation end to end through rustls handshakes
- add the web-faith-revocation crate
