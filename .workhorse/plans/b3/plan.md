# Skip CI for the guard's artefact-cleanup commit

The Workhorse guard writes a commit removing `.workhorse/plans/`, `.workhorse/breakdowns/`, and `.workhorse/test-cases/` for the card, then merges.
That commit is a new head SHA, so the full test matrix and the conformance job run again before the merge is allowed.
Every cleanup commit in the history so far is pure deletions confined to those three trees, so the second run covers a code tree identical to the one already tested.

## Why the obvious approaches do not work

**Trigger-level `paths-ignore` cannot see the cleanup push.**
For `pull_request` events GitHub evaluates path filters against a three-dot diff, comparing the PR head against the merge base rather than against the previous head.
The PR as a whole still changes Rust and JS, so the filter never matches and the workflow runs anyway.
This is also why the "would an agent adding a plan.md exempt itself from CI" worry does not arise from trigger filters: only a PR whose entire diff is artefacts would skip.

**A workflow skipped by a trigger filter hangs a required check.**
The check stays Pending as "Expected, waiting for status to be reported" and blocks the merge indefinitely.
GitHub's own guidance is to avoid requiring workflows that can be skipped.
A job skipped by an `if:` condition behaves the opposite way and reports success, which is the mechanism this design uses.

**Path filters cannot express "deletions only".**
They match filenames and nothing else.
A job can express it, because a job is a script with the two commits in hand.

## Approaches ruled out

Folding the removal into the last agent commit defeats the point: the cleanup is deliberately written only once CI is green.

Cleaning up after the merge, in a commit direct to `main`, is a non-starter.

Building the removal into the merge commit's tree is a non-starter.

A merge queue does not help.
The same required checks gate entry to the queue, not just the merge, so the cleanup commit's SHA still needs green checks before it can be enqueued.

## Design

A `gate` job in each workflow decides whether the real work can be skipped, and the expensive jobs take `needs: gate` with `if: needs.gate.outputs.skip != 'true'`.

The predicate lives in one place, `.github/scripts/cleanup-only-gate.sh`, so the two workflows cannot drift on the security-relevant part.
The gate job checks out the PR head at `fetch-depth: 2`, which supplies both the script and the two commits it compares.

The skip requires all of:

- The event is `pull_request`.
  On `push`, `schedule`, and `workflow_dispatch` the gate produces no output and the work runs.
- `git diff --name-status HEAD^ HEAD` is non-empty and every line is a deletion under `.workhorse/plans/`, `.workhorse/breakdowns/`, or `.workhorse/test-cases/`.
  Specs are excluded on purpose: a spec-only change is a real change to the product's information architecture and has no green parent licensing a skip.
- The parent commit has the workflow's own required check recorded as `success`.

That last condition is what makes the skip sound rather than merely convenient.
Required checks are evaluated on the latest commit SHA only, so without it a hand-made sequence of "push code, then push an artefact-deletion commit" would present a green head over untested code.
Requiring the parent to be green means the skip inherits a real result instead of inventing one.

The gate fails closed.
An unreadable diff, an API error, a parent whose result was cancelled or is still pending, or anything outside the artefact trees all fall through without setting an output, and the work runs.

### Wiring per workflow

Three workflows trigger on `pull_request`, and all three are required, so each needs its own gate.
Each gate reads its own workflow's required check on the parent, never another workflow's, so a green conformance run cannot license skipping the tests.

`test.yml` reads `Tests pass` and emits `allowed-skips: test`.
The existing `tests-pass` job already threads `allowed-skips` into `re-actors/alls-green`, but it reads `needs.plan.outputs.allowed-skips` and there is no `plan` job in the workflow, so that expression is dead template leftover.
Pointing it at the gate makes it live, which matters because `alls-green` treats a skipped dependency as a failure unless the job is listed.

`publish.yml` reads `Builds pass` and emits `allowed-skips: build, build-freebsd, publish`.
All three, because `publish` depends on the two build jobs and so is skipped transitively when they are.
`builds-pass` carries the same dead `needs.plan.outputs.allowed-skips` reference and needs the same repointing.

`conformance.yml` reads `conformance matrix` and gates that job directly.
It has no `alls-green` wrapper, so this is the one place that relies on a job skipped by `if:` reporting as success to branch protection.
Worth confirming on a throwaway PR before trusting it.
If it does not hold, the fix is an `alls-green` wrapper job, which changes the required check's name and so needs a branch protection edit too.

`publish.yml` also triggers on version tags, where the gate must stay out of the way.
The decide step is confined to `pull_request`, so on a tag the gate job is a bare checkout that succeeds and the release builds run.
The cost is a short serial checkout ahead of the expensive jobs on every run, including release tags and the nightly conformance cron.

### What the second run actually costs

Measured from one representative successful `pull_request` run of each workflow, rather than inferred from job counts.

| Workflow | Jobs | Slowest job | Job-minutes |
| --- | --- | --- | --- |
| `test.yml` | 13 | 5 min | 52 |
| `publish.yml` | 14 | 5 min | 23 |
| `conformance.yml` | 1 | 2 min | 2 |

Job count and exotic targets are a bad proxy for cost, in both directions.
`publish.yml` has a FreeBSD VM and eleven cross-compiles and still costs less than half of `test.yml`, because its builds are cached compiles while every one of the twelve test cells installs Go, builds go-httpbin, installs Caddy, and then runs the suite.
`conformance.yml` looks like the heaviest job in the repository and finishes in two minutes.

So the three gates are worth having for different reasons, and the ordering is not the one the workflow files suggest.
The wall-clock delay a cleanup commit adds before the merge is set by whichever workflow is slowest, currently a tie around five minutes of job time plus queueing.
The billable total is roughly 77 job-minutes per cleanup commit, three quarters of it in `test.yml`.

### Check names

`main` has no branch protection; the gating comes from a ruleset named "PRs required" on `~DEFAULT_BRANCH`, whose `required_status_checks` rule lists exactly three contexts: `Builds pass`, `Tests pass`, and `conformance matrix`.
Those are the three strings the gates pass as `REQUIRED_CHECK`.
Note the third is the `matrix` job's `name:`, not the workflow's: a workflow name is not itself a check run.

The exact string matters in one direction.
A `REQUIRED_CHECK` matching nothing is safe, because the gate then never skips.
A name that matches a check run nobody actually requires would license a skip on a result nobody gates on.

The ruleset sets `strict_required_status_checks_policy: false`, so a branch does not have to be current with `main` to merge.
Nothing in this design depends on that either way, but a later switch to strict would add a rebase before merge, which is a new head SHA that is not a cleanup commit and so correctly runs everything.

## Build

- [x] Confirm the three required check names against the "PRs required" ruleset
- [x] Add `.github/scripts/cleanup-only-gate.sh` with the predicate, fail-closed on every ambiguity, committed executable
- [x] Add the `gate` job to `test.yml`, with `permissions: contents: read` and `checks: read`
- [x] Put `needs: gate` and the `if:` guard on `test`, and repoint `tests-pass` at `needs: [gate, test]` with the gate's `allowed-skips`
- [x] Add the `gate` job to `publish.yml`, guard `build` and `build-freebsd`, and repoint `builds-pass` with `allowed-skips` covering `publish` too
- [x] Add the `gate` job to `conformance.yml` and guard the `matrix` job
- [x] Exercise the predicate locally against the real cleanup commits with a stubbed `gh`, covering green, failed, cancelled, absent, ambiguous, and API-error parents
- [x] Exercise the shapes that must not skip: artefact deletions plus a `src/lib.rs` edit, an added plan, an edited plan, a deleted spec, an empty commit
- [ ] Confirm on this card's own PR that an artefact-deletion commit skips and all three checks still report green
- [ ] Confirm a version tag still builds and publishes with the gate in the graph
