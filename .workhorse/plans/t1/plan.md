# Spec the current state of Fáith

## Scope of the spec set

The specs cover the library's own behaviour: the fetch surface, the agent and its sub-configuration, responses, HTTP/3, errors, and environment variables. The test apparatus is out of scope. An earlier pass specced the conformance matrix and the benchmark harness; those were dropped as over-specification, since neither describes what the library requires of an implementation.

## Describing behaviour that is known to be wrong

The specs describe Fáith as it behaves today, including behaviour that is a defect. Where a spec section records something the follow-up work will change, the breakdown entry for that work names the spec section, so the pair stays reviewable and no spec sentence is left quietly aspirational.

## Where upstream limitations are recorded

Spec text states behaviour without attributing it to a dependency's constraints, per the workspace spec rules. The causes live in `.workhorse/upstream-limitations.md`, one entry per limitation naming the spec section it shapes. That keeps the attribution out of the specs while leaving a list to re-check when the HTTP stack is upgraded.

## Relationship to the upstream standards

The overview names the standards Fáith answers to and states the rule: a departure is either a browser concept with no server-side meaning or a deliberate divergence named in the spec that covers it, and anything else is a defect. Two deliberate divergences are recorded that way so far, method uppercasing (REQ) and any-match integrity verification (SRI), both with follow-up cards to bring them back to the standard.
