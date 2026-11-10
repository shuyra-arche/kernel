# RFC: <short, precise title>

- RFC: <assigned number, after allocation>
- Status: Proposed
- Related Issues: <links>
- Author: <author or team>
- Created: <YYYY-MM-DD>
- Supersedes: None

## Problem

Which architectural problem or conflict requires a decision?
Describe the current state without assuming that a proposal is implemented.

## Goals and non-goals

What must change, and what remains outside this decision?

## Invariants and trust boundary

Who owns each authoritative fact? Which permission, lifecycle, identity,
or hardware-completion conditions must never be bypassed?

## Proposal

Explain the model, relevant states, legal transitions, and public effects.
Include diagrams only where they make a boundary or invariant clearer.

## Alternatives

Describe viable alternatives and why each was or was not selected.
Include the option of keeping the existing design when relevant.

## Costs and failure modes

What complexity, performance, migration, concurrency, safety, or
recovery costs does this introduce? What happens after partial failure,
timeout, or caller death?

## Compatibility and migration

Which API/ABI contracts, consumers, boot formats, or integrations change?
State the order in which providers and consumers must be updated.

## Validation

Which invariant or observable contract would demonstrate correctness?
Separate Host, target compilation, QEMU, and physical hardware evidence.

## Open questions

List unresolved decisions explicitly. Do not turn them into hidden
implementation commitments.

## Decision and implementation record

Complete this section when reviewed:

- Decision: <accepted, rejected, or deferred; date and rationale>
- Implementation PRs: <links, or pending>
- Contract updates: <links, or none>
- Verification evidence and remaining limits: <links or summary>
- Replacement RFC: <link, if superseded>
