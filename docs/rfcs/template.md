# RFC [GitHub Issue number]: [Short, precise title]

- **Tracking Issue:** [Link to the numbered GitHub Issue]
- **Areas:** [Relevant architectural concerns, not source directories]
- **Supersedes:** None

> Drafts are reviewed as pull requests. A document merged into `docs/rfcs/`
> records an accepted direction, not a claim of completed implementation.
> Do not manually duplicate decision or implementation status here.

## Problem

Which architectural conflict or unmet requirement requires a decision?
Describe the current contract honestly, including what is not implemented.

## Goals and non-goals

What must change, and which responsibilities are deliberately unchanged?

## Invariants and trust boundary

Who owns each authoritative fact? Which permission, identity, lifetime,
ordering, or retirement conditions must remain true after the change?

## Proposal

Define the model, states, legal transitions, effect boundaries, and
externally observable changes. Prefer a focused diagram only when it
makes a boundary or invariant clearer.

## Alternatives

Describe credible alternatives, including keeping the present model.
Explain why the proposal is preferable and under which assumptions.

## Costs and failure modes

What new complexity or performance cost does the model introduce?
Who remains responsible after partial failure, cancellation, stale
completion, timeout, or caller death?

## Compatibility and migration

Which public API/ABI, boot contracts, providers, or integrations change?
Link to the affected work and record an ordered migration if needed.

## Validation and acceptance

What observable properties and failure cases must be verified?
Distinguish Host, target compilation, QEMU, and real hardware evidence.
Do not claim the entire RFC was implemented merely because a linked
implementation PR merged.

## Open questions

List any decisions that must be settled before acceptance. Do not treat
an unresolved issue as an implicit implementation promise.

## References

Link the proposal Issue, previous RFCs and related PRs. Implementation
progress and verification evidence live in linked Issues/PRs and the
contract/tests that own the behavior, rather than in an RFC status diary.
