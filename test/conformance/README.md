# The conformance harness

The rest of `test/` asks whether faith does what faith says. This asks something narrower and more
awkward: whether faith is *right about the wire*. Trailers, chunked framing, content codings — the
parts of HTTP where a client can be plausibly wrong and still look fine, because nothing it talks to
ever exercises the edge. go-httpbin cannot emit trailers on demand and will not lie about a
Content-Encoding, so those behaviours went untested until there was an origin here that could.

## The capability model

Each server declares a `capabilities` Set; each dimension declares the `requires` it needs. The
runner takes the cross product and runs a cell only where the requirements are met, skipping the
rest with the reason attached. Nothing hand-writes which combinations make sense, because a
hand-written matrix drifts the first time a server changes — you would end up with a table that
claims coverage the servers can no longer provide.

The names live in one frozen object in `capabilities.js`, and a name outside it throws. That
matters more than it looks: a typo in a `requires` list would otherwise skip every cell mentioning
it, and a suite with nothing to do is indistinguishable from a suite that passed.

## Running it

```bash
npm run test:conformance
```

That runs the servers' own selftest first (the origin is test infrastructure, so it needs testing
too — every dimension's verdict rests on it serving what it claims) and then the matrix runner.
`npm run test:conformance:servers` runs just the selftest. No build step is needed beyond whatever
you already built.

## Adding a dimension

Write a module under `dimensions/` exporting `name`, `requires`, an async `run(t, ctx)`, and the
`assertions` count that `run` makes. `ctx` gives you `url` and a pre-configured `agent` that trusts
the test CA. Add an optional `negative(t, ctx)` — plus its `negativeAssertions` count — for the case
where the server misbehaves on purpose; it runs only against `SCRIPTABLE` servers. A dimension
without a negative case should say in its header comment why it does not need one.

The declared counts are load-bearing. tape treats a test that asserts nothing as a pass, so a
dimension whose body was gutted in a refactor would go green; the runner compares the count it
actually saw against the one you declared, which makes coverage both lost and gained fail loudly.
Add the module to `DIMENSIONS` in `run.js`, then update `expected-matrix.json`.

## Adding a server

Export an object with `name`, `expectVersion`, `capabilities`, and an async `start()` returning
`{ url, ca, close }`. Only claim a capability the server genuinely has: an h2-only listener must not
claim `CHUNKED`, because HTTP/2 has no chunked encoding, and claiming it would make the framing
dimension run a test that cannot mean anything. `expectVersion` is asserted once per cell — nothing
in the dimensions is version-specific, so without that probe a row named `-h2` could quietly run
every one of its assertions over HTTP/1.1. Add it to `SERVERS` in `run.js`, and to
`expected-matrix.json`.

## expected-matrix.json

The checked-in shape of the matrix: every cell, its status, and its skip reason. The runner asserts
the computed matrix against it, because a cell that silently disappears — a capability declaration
edited, a server that stopped starting — looks exactly like a clean run otherwise. The reason is
compared as well as the status, since a cell skipping for a *different* reason than intended is
coverage quietly reduced, and both a status-only comparison and a passing suite would accept it.

You edit this file by hand and commit it; the runner only ever reads it. When the assertion fails,
decide whether the coverage change was intended before editing the file to match.

## matrix.json

The runner writes this; it is git-ignored and nobody edits it. One write, when the run finishes, with
`"kind": "realised"` and a per-cell `outcome` of `pass`, `fail` or `skipped`.

`status` and `outcome` answer different questions. `status` is the capability model's decision about
whether a cell *should* run; `outcome` is what the run did. A cell can be `"status": "run"` and
`"outcome": "fail"`, and a consumer conflating them would publish a failed row as verified.

The runner emits the outcomes because it is the only thing that knows them. Anything that renders
this — into the top-level README, say — is a consumer and should not re-derive or re-run to find out
what happened.

In CI the file lives on ephemeral runner storage, so a workflow that wants it must upload it as an
artifact with `if: always()`, or it is gone with the runner.
