# Kernel RFCs

An RFC records a consequential **design decision** and the reasons for it.
It is not an implementation checklist, a duplicate API specification, or a
separately maintained status database.

## GitHub is the workflow record

Use GitHub's existing identities and relationships instead of maintaining
parallel numbers, indexes, and progress fields by hand:

| Question | Authoritative source |
| --- | --- |
| What number does the RFC have? | The GitHub Issue number allocated when the proposal is opened |
| What is still proposed? | Open RFC discussion / RFC pull requests |
| What has been accepted? | RFC documents merged into `main` by an explicit design-review decision |
| What was rejected or withdrawn? | The original Issue and closed, unmerged proposal PR |
| What does an RFC replace? | The newer accepted RFC's `Supersedes` reference to the older Issue |
| What implementation work exists? | Cross-referenced implementation Issues and pull requests |
| What is actually implemented and verified? | The owning API, tests, integration evidence, and reviewed implementation scope |

**Do not store a manually maintained `decision` or `implementation` status
in an RFC front matter.** These are different facts with different owners.
A merged code PR is not a universal proof that the complete design was
implemented or verified.

The GitHub repository already provides a browseable, current file listing in
`docs/rfcs/`. Avoid a second numbered `INDEX.md` and per-area directory
trees requiring synchronized state. A richer generated catalogue may be added
only when real navigation needs justify it.

## Identity and files

First open a GitHub Issue describing the architectural question. Use its
GitHub Issue number as the RFC number: for example, Issue #124 would be
`0124-external-access-authority.md`, displayed as `kernel/RFC-0124`.
Four-digit zero-padding is only a sorting convention; the Issue number is
the authoritative identity. Do not maintain a separate RFC counter or
reassign a number after a rejection.

Prepare the RFC using [the template](template.md) in a dedicated PR. The
proposal PR itself is the live draft, not an already accepted RFC. Its
review history records debate and trade-offs. An explicit decision to merge
the RFC means the proposed direction is accepted; it does not imply
implementation.

A declined proposal stays discoverable via its Issue and closed PR. There
is no need to merge rejected content into the accepted-design directory
merely to preserve a status row.

## Revision and replacement

- The **accepted decision text** remains a historical record. Editorial
  corrections may be made, but a fundamental reversal needs a new RFC.
- A new RFC records the IDs that it supersedes and links to their Issues.
  GitHub's cross-reference history preserves the reverse relationship
  without requiring edits to every predecessor.
- A replacement does not rewrite history. It must explicitly name which
  constraints or decisions are being superseded.
- Implementation PRs reference the RFC's Issue (without using an accidental
  auto-close keyword for partial work). Reviewers decide when the overall
  acceptance and verification criteria have been met.

This avoids three sources of truth: the RFC body for the design decision,
the Issue/PR graph for work and decisions, and the API/tests for what
actually works. Each fact has one owner.

## When an RFC is appropriate

Use an RFC when a change affects foundational abstractions, isolation,
authorization, invariant ownership, lifecycle/retirement, or the public
semantic contract and has meaningful alternatives.

For a bug fix, source layout change, or routine implementation task, prefer
an Issue and PR without adding an RFC.

## Automation without Kernel CI

GitHub automatically allocates Issue numbers, records discussion, links
mentions, and tracks whether PRs are open, merged, or closed. These native
records form the live RFC catalogue; they do not require separate doc
commits to synchronize metadata.

Additional tooling can validate RFC filenames, headings, Issue references,
and supersession links or generate a rendered catalogue **from** these
canonical records. Such tooling should be a small RFC-specific maintenance
action, not the Kernel RV64/QEMU check workflow. Pure documentation pushes
must not launch Kernel build/guest tests; see
[CI PR #126](https://github.com/shuyra-arche/kernel/pull/126).

Do not create a bot that automatically accepts a design, infers full safety
verification solely from merged PRs, or silently edits architectural
conclusions. Those are review decisions.

## Relation to other documentation

- [Current design](../design/main.typ) explains why the system is coherent.
- [Kernel API](../../api/README.md) defines the current public contract.
- RFCs preserve accepted design rationale and its alternatives.
- Issues and PRs record workflow, implementation evidence, and unresolved work.

For example, [Kernel #124](https://github.com/shuyra-arche/kernel/issues/124)
may motivate a Port-boundary RFC. The example number is not a claim that such
an RFC has already been accepted.
