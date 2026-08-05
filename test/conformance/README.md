# The conformance harness

Read `run.js` for how the matrix is computed, an existing module under `dimensions/` or `servers/` for
the shape of one, and `package.json` for the commands. Rationale for the harness existing, and the
remaining phases, are on issue #25.

What follows is only the things you cannot get from the code.

## Prove a new dimension can fail

Before committing one, break the thing it checks — flip an expected value, gut the `run` body, point
it at a route that misbehaves — and watch it go red. Then revert. Every dimension here has been
through that, and three of them were wrong the first time in ways a passing suite could not show:
assertions that held for both correct and broken behaviour. A conformance test nobody has seen fail is
decoration.

## Only declare capabilities a server genuinely has

The matrix is derived from those declarations, so a wrong one does not fail — it silently runs a test
that cannot mean anything, or skips one that could. If a dimension has no `negative` case, say why in
its header comment, so the next person can tell a deliberate omission from an oversight.

## A failing matrix assertion is a question, not a chore

When the computed matrix stops matching `expected-matrix.json`, work out whether the coverage change
was intended before editing the file to match. Editing first defeats the only guard against coverage
quietly shrinking.

## In CI, `matrix.json` needs uploading

It is written to the runner's ephemeral storage, so a workflow that wants it must upload it as an
artifact with `if: always()` — otherwise it goes with the runner, including on exactly the failing
runs where it is most worth having.
