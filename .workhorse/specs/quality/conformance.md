---
id: CONF
---

# Conformance matrix

Fáith's correctness claim rests on a conformance suite that exercises protocol features against real server implementations, not just mocks: whatever servers emit, Fáith must handle.
The suite is a matrix of servers (rows) by protocol dimensions (columns), and its realised outcome is rendered into the README as the public statement of coverage.

## Structure

The matrix is derived, never hand-maintained: every server declares the capabilities it has, every dimension declares the capabilities it requires, and their intersection plans which cells run and which skip (with the missing capability as the recorded reason).
Capability names come from a single closed vocabulary; declaring an unknown capability anywhere is an immediate error, so rows and dimensions cannot drift apart silently.
The computed plan is compared against a committed expected matrix: a cell appearing, vanishing, or changing its skip reason fails the run, so coverage changes are always deliberate.
Server availability deliberately does not affect planning, so the plan is identical on every machine.

## Server rows and dimensions

Rows cover distinct HTTP implementations and topologies: in-process Node origins (HTTP/1.1 and HTTP/2), Caddy, nginx, Apache (HTTP/1.1 and HTTP/2), HAProxy as a true proxy hop in front of the Node origin (HTTP/1.1 and HTTP/2), and an HTTP/3-only row with no TCP listener, reached via an Alt-Svc hint.
Three independent QUIC server stacks are represented across the rows.
Dimensions cover trailers, chunked bodies, gzip, conditional GET, ALPN protocol negotiation, connection reuse under server keep-alive limits, oversized-header rejection, HTTP/2 GOAWAY handling, HTTP/3, and the HTTP/3 Alt-Svc upgrade.
Each row is verified to actually serve the shared route contract it declares (in its own selftest), and each dimension asserts a declared assertion count so silently lost coverage fails the run.
Before a row's dimensions run, a version probe asserts the row negotiated the protocol version it exists to represent.

## Missing binaries versus missing capabilities

A capability a server lacks is a planned skip.
A server binary that is missing or unusable on this machine marks its cells **unavailable**, a distinct third outcome that cannot be misread as either skipped or passed, with a reason distinguishing "not installed" from "installed but built without the needed feature".
On developer machines, unavailable rows are tolerated and summarised; in CI, a strict mode makes any unavailable row a failure, so the canonical run always covers every row.

## Rendering and CI

The runner emits the realised matrix (every cell with its outcome) as an artefact, and a renderer splices the table between the conformance markers in the README, showing pass, skip, fail, and unavailable distinctly and only the legend entries present.
CI runs the matrix strict on pushes, pull requests, and a nightly schedule (the rows exercise third-party servers, which can regress on their own timeline), provisioning pinned versions of each server, and fails if the committed README table does not match the freshly realised one.
