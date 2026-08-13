#!/usr/bin/env bash
# Is HEAD a card-artefact cleanup commit that a green run already covers?
#
# The Workhorse guard writes that commit only once CI has gone green, then merges.
# Its code tree is identical to the tree that was just tested, minus some deleted
# markdown, so building and testing it again buys nothing -- but it is a new head
# SHA, and required checks are evaluated on the head SHA alone.
#
# Decided here rather than with `paths-ignore` for two reasons. A trigger filter on
# a pull_request event sees the whole PR diff (three-dot, against the merge base)
# rather than this push, so it would never match. And a workflow skipped by a
# trigger filter never reports at all, which leaves a required check Pending and
# hangs the PR forever; a job skipped by an `if:` condition reports success.
#
# Callers set REQUIRED_CHECK to their own workflow's required check, never another
# workflow's, so a green conformance run cannot license skipping the tests.
#
# Every branch below is a reason to run anyway. Fail closed: an unreadable diff, an
# API error, a parent that was cancelled or is still pending, anything touching a
# path CI depends on -- all return without setting an output, and the caller runs.
set -euo pipefail

# Captured rather than piped into grep: `git diff | grep -q` leaves git holding a
# closed pipe, and under pipefail that SIGPIPE outranks grep's own status and
# inverts the decision.
changes=$(git diff --name-status HEAD^ HEAD)

if [ -z "$changes" ]; then
  echo "HEAD changes nothing; running"
  exit 0
fi

# Deletions only, and only the three card-scoped artefact trees. Specs are left out
# on purpose: a spec-only change is a real change to the product's information
# architecture, and no green parent licenses skipping it.
if grep -qvE '^D[[:space:]]+\.workhorse/(plans|breakdowns|test-cases)/' <<<"$changes"; then
  echo "HEAD is not an artefact-cleanup commit; running"
  exit 0
fi

parent=$(git rev-parse HEAD^)

# The check name comes from the environment because gojq reads `env`, which beats
# quoting it into the filter. Paginated because a commit here carries the better
# part of forty check runs across the three workflows.
if ! conclusion=$(gh api --paginate \
  "repos/${GITHUB_REPOSITORY}/commits/${parent}/check-runs" \
  --jq '.check_runs[] | select(.name == env.REQUIRED_CHECK) | .conclusion'); then
  echo "cannot read checks for ${parent}; running"
  exit 0
fi

# Exact match against a single line, so absent (empty) and ambiguous (more than one
# run under that name) both fall through to running.
if [ "$conclusion" != "success" ]; then
  echo "${REQUIRED_CHECK} on ${parent} is '${conclusion:-absent}', not success; running"
  exit 0
fi

echo "${REQUIRED_CHECK} is green on ${parent} and HEAD only deletes card artefacts; skipping"
echo "skip=true" >>"$GITHUB_OUTPUT"
echo "allowed-skips=${SKIPPABLE_JOBS:-}" >>"$GITHUB_OUTPUT"
