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

`test.yml` reads `Tests pass` on the parent and has the gate emit `allowed-skips: test`.
The existing `tests-pass` job already threads `allowed-skips` into `re-actors/alls-green`, but it reads `needs.plan.outputs.allowed-skips` and there is no `plan` job in the workflow, so that expression is dead template leftover.
Pointing it at the gate makes it live, which matters because `alls-green` treats a skipped dependency as a failure unless the job is listed.
`Tests pass` therefore genuinely runs and reports green; nothing here depends on how branch protection treats a skipped check.

`conformance.yml` reads `conformance matrix` on the parent and gates that job directly.
It has no `alls-green` wrapper, so this one does rely on a job skipped by `if:` reporting as success to branch protection.
Worth confirming on a throwaway PR before trusting it.
If it turns out not to hold, the fix is an `alls-green` wrapper job, which changes the required check's name and so needs a branch protection edit too.

Adding `needs: gate` puts a short serial checkout ahead of the expensive jobs on every run, including the nightly conformance cron.

### Open question

Which checks are actually required on `main` is not visible from the worktree, and the names are load-bearest: a `REQUIRED_CHECK` string that matches nothing means the gate never skips, which is safe, but a stale name that matches a check nobody requires would license a skip on a result nobody gates on.
Confirm the exact required check names before wiring.

## Build

- [ ] Add `.github/scripts/cleanup-only-gate.sh` with the predicate, fail-closed on every ambiguity, committed executable
- [ ] Add the `gate` job to `test.yml`, with `permissions: contents: read` and `checks: read`
- [ ] Put `needs: gate` and the `if:` guard on `test`, and repoint `tests-pass` at `needs: [gate, test]` with the gate's `allowed-skips`
- [ ] Add the `gate` job to `conformance.yml` and guard the `matrix` job
- [ ] Verify on a throwaway PR: a code push runs everything, an artefact-deletion commit on top skips and still reports green, and an artefact-deletion commit whose parent is red or cancelled runs anyway
- [ ] Verify a deletion commit pushed with no prior run at all does not skip
