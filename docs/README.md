# Kernel documentation

This directory explains **why** the Kernel is designed as it is. It should
help a reader understand the isolation and composition model without requiring
them to reconstruct its invariants from source code.

## Start here

1. [Kernel](../README.md) — current project scope, source map, build and test
   entry points.
2. [Architecture](architecture/README.md) — design goal, system boundary, and
   how to navigate the model.
3. [Core model](architecture/model.md) — identities, ownership, authorization,
   publication, and settlement.

## Where each kind of truth lives

| Question | Canonical owner |
| --- | --- |
| Why does the Kernel have this boundary or invariant? | `docs/architecture/` |
| What public operation, type, layout, error, or compatibility rule exists **now**? | [`api/README.md`](../api/README.md) and [`api/src/`](../api/src/) |
| How does a typed caller invoke the Kernel? | [`client/README.md`](../client/README.md) and `client/src/` |
| How does the Server enforce an invariant? | Module documentation and code under [`server/src/`](../server/src/) |
| What behavior has been verified, on which target? | Tests and scenario notes under [`server/tests/`](../server/tests/) and recorded CI evidence |
| What is undecided or being replaced? | A linked GitHub Issue or PR, not a speculative ABI reference |
| How do contributors change the repository? | [`CONTRIBUTING.md`](../CONTRIBUTING.md) and the [organization standards](https://github.com/shuyra-arche/.github/tree/main/docs/engineering) |

An architecture document explains stable responsibilities, invariants,
trust boundaries, and the evidence required for completion. It should not
reproduce the whole call table or record every temporary implementation step.
A public contract belongs beside the API that owns it, rather than in a
second `docs/contracts/` tree. Implementation details stay near their
enforcing modules, rather than in a source-directory mirror under
`docs/implementation/`.

## Architecture topics

The current overview pages are:

- [Architectural introduction](architecture/README.md)
- [Core model and safety invariants](architecture/model.md)

Additional topics should become pages **only when there is substance to
document and an independent responsibility to explain**:

| Intended page | Question it should answer |
| --- | --- |
| `architecture/memory.md` | How do Page ownership, Flat holdings, Nest associations, Cell positions, and retirement interact? |
| `architecture/interaction.md` | How do View, Pend, and Sign authorize, move, share, and observe flows across isolation boundaries? |
| `architecture/execution.md` | How do Team, Task, and Room manage execution, waiting, results, and cleanup? |
| `architecture/hardware.md` | Which MMU/IOMMU, interrupt, and platform facts and completion proofs does the Kernel trust? |

These are a navigation plan, **not** empty documentation commitments or
evidence that unimplemented behavior already exists. Do not create placeholder
files solely to complete a directory tree.

## Change discipline

- Keep **one authoritative description** for every fact, with links where other
  documents need it. Remove stale copies rather than maintaining duplicates.
- Mark implemented behavior, enduring invariants, and open proposals separately.
  A model is not automatically an implemented ABI or a hardware guarantee.
- Update the API contract and affected tests when observable behavior changes.
  Update architecture only when a responsibility, boundary, or invariant changes.
- Use Issues for alternatives and unresolved decisions, PRs for change rationale,
  and CI artifacts for raw execution evidence.
- Keep headings and names stable and links relative where possible. Favor
  short explanations and focused diagrams that make the enforcing owner clear,
  not an ever-growing chronological design diary.
