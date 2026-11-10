# Kernel RFCs

An RFC records a **proposed architectural decision** and its rationale.
It is distinct from the current design document, the public API contract,
and a task-tracking Issue.

Use an RFC when the change alters a foundational abstraction, trust
boundary, invariant owner, or externally observable semantic contract
and reasonable alternatives deserve explicit consideration.

A focused bug fix, internal file move, ordinary documentation improvement,
or implementation task usually needs only an Issue and PR.

## Lifecycle

| Status | Meaning |
| --- | --- |
| Proposed | Open for design review; not an accepted contract |
| Accepted | Direction agreed; implementation and verification may still be pending |
| Rejected | Considered but deliberately not adopted |
| Implemented | Accepted direction has landed, with linked evidence and any remaining limits |
| Superseded | A newer RFC explicitly replaces the decision |

Acceptance and implementation are separate. Merging a proposal into
`rfcs/` does **not** imply that its behavior exists in the Kernel.
An RFC should link the adopting code PR and applicable contract updates
when implementation completes.

## Format and review

Use `NNNN-short-topic.md`, with monotonically increasing four-digit
identifiers. Begin from [the template](template.md); write in Markdown so
reviewers can inspect changes and discuss them directly on GitHub.
Do not create numbered placeholders.

A proposal should state the problem, existing invariants, intended change,
alternatives, trade-offs, compatibility or migration, and criteria for
acceptance. Link relevant Issues and review decisions; do not use an RFC
to duplicate raw build logs.

Keep the original rationale discoverable. Prefer a follow-up RFC when a
previously accepted decision is fundamentally replaced, setting the
older document to `Superseded` and linking its replacement.
Small editorial corrections are fine.

## Relationship to the design

- [Typst design](../design/main.typ): the current coherent explanation of
  **why** the Kernel's architecture works; not a design history.
- [API contracts](../../api/README.md): the authoritative representation
  of what callers can rely on **now**.
- RFC: what was proposed, which alternatives were considered, and **why**
  the design changed.
- Issue / PR: the work, discussion, migration steps, and actual verification.

For example, the question of retiring public Port in
[Issue #124](https://github.com/shuyra-arche/kernel/issues/124) is a
potential RFC because it changes an authorization boundary. No
Port-removal RFC has yet been accepted here.
