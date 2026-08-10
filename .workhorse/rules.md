# Spec rules

Specs live in `.workhorse/specs/<area>/<name>.md`.
This file is the house style for this workspace; it sits on top of the Workhorse spec conventions in `.agents/docs/spec-format.md` and wins wherever the two differ.

A spec is the durable description of **what** the system requires.
It is read by someone deciding whether the implementation is correct, or re-implementing the feature from scratch — not as a narrative of how the code works or how it came to be.

Spec ids, frontmatter, and the fold-vs-create-vs-split decision follow `.agents/docs/spec-format.md`.
The spec-first workflow — spec before implementation before tests — is carried by the Workhorse skills (Draft spec changes, Implement this, Draft test cases); it is not restated here.

## Prose, not checklists

Specs are written in markdown prose with each sentence on its own line and no hard-wrapping.
This balances ease of writing and diff parseability.
Acceptance criteria are prose sentences rather than `- [ ]` checklist items; this overrides the checklist format shown in `.agents/docs/spec-format.md`.

## Cross-references

Link a spec to a related spec — or reference one from any other markdown — by its path under its id: `[BAK](../public-server/backup.md)`.
From code, tie an implementation back to the specific requirement it answers with an inline `// spec:ID#fragment` comment, where `fragment` is the slug of the heading the requirement sits under: `// spec:BAK#retention`.
The reference contains no whitespace.
Always deep-link to a section; referencing a whole spec (`spec:BAK`) is not allowed.

## What, not how

- Describe **what** the system requires, not **how** the code achieves it.
  Keep out of spec text: tool and command names (`sfdisk`, `kopia snapshot create`), crate and library names, syscall names (`splice(2)`), internal API details, data-structure choices, and environment variable names used only by the implementation.
- Acceptable, because external actors or other components depend on them: interface contracts — config file paths and formats, on-disk and on-the-wire shapes, endpoint shapes, partition UUIDs, credential scopes.
- The test: would someone re-implementing the feature from scratch be constrained to the same choice?
  If not, it's an implementation detail and doesn't belong in the spec.

## Present, not past

- State what the system does, not how it got there.
  No "this supersedes X", "formerly Y", "a spike settled Z", changelog entries, or migration narration.
- When something is removed or replaced, edit the spec to describe the new reality and delete the old text, rather than describing the transition.
  The git history is the record of change; the spec is the record of the present.

## Own behaviour, not a dependency's internals

- Describe the system's own behaviour and contracts: the request shape it handles, the guarantee it makes.
  Don't narrate a dependency's decision logic or version-specific quirks beyond the minimum needed to justify a requirement.
- Where behaviour is shaped by what a dependency can express, state the behaviour in the spec without attributing it, and record the cause in [`upstream-limitations.md`](upstream-limitations.md).
  A caller depends on the behaviour either way; the register is what makes it reviewable when the dependency changes.
- Don't scaffold or label: no "Strategy A/B", "Phase N", or plan tags in spec prose.
  Describe the mechanism directly.
